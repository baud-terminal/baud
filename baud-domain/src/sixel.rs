//! The Sixel raster decoder: the bytes inside a `DCS ... q ... ST` turned into
//! a paletted bitmap. Pure, `no_std`, and it allocates nothing -- the caller
//! owns the pixels, through the image store's arena a terminal decodes into as
//! the body arrives ([`SixelStream`]) or a [`SixelCanvas`] over a slice of its
//! own, exactly as the caller owns the grid and the arena.
//!
//! **One grammar, a byte at a time.** The walk keeps what a body leaves
//! between two bytes -- the registers, the position, the command and number
//! in progress -- so a body split anywhere, a number included, decodes as the
//! whole does, and no copy of it is kept. Where a set bit lands is a sink's:
//! nothing (measuring), a canvas the caller sized, or the gap of the arena a
//! Sixel claimed at its `q`.
//!
//! The grammar is `[[source: vt3xx-sixel]]` -- the VT330/VT340 manual chapter
//! `[[source: ctlseqs]]` names and defers to. The DCS *envelope*
//! (`DCS Pa ; Pb ; Ph q`) is the parser's, not this module's; this decodes the
//! data that follows the `q`.
//!
//! Only the parts a decoder implements are here: the data bytes, the colour
//! registers, the repeat introducer, graphics carriage return and new line,
//! and the raster attributes; `baud-domain/tests/sixel_decode.rs` is the
//! gate its decode answers to.

use crate::decode::{InputEncoding, StreamByte};
use crate::image::{
    ImageAlpha, ImageGround, ImageId, ImagePx, ImageRefusal, ImageStore, Pixel, Plane,
};
use crate::style::Chan;

/// A pixel coordinate or extent. A Sixel is bounded by its raster attributes
/// and by the caller's canvas; `u16` covers every image a terminal draws and
/// keeps the type small.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct SixelPx(u16);

impl SixelPx {
    /// The origin.
    pub const ZERO: SixelPx = SixelPx(0);

    /// A coordinate or extent of `n` pixels.
    pub fn of(n: u16) -> SixelPx {
        SixelPx(n)
    }

    /// As an index into a caller slice.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The raw pixel count, for a caller sizing an image arena from a report.
    pub fn raw(self) -> u16 {
        self.0
    }

    /// A walk position as a coordinate, or `None` past what a `u16` holds.
    fn at(v: u32) -> Option<SixelPx> {
        u16::try_from(v).ok().map(SixelPx)
    }
}

/// Whether an image's extent fits a [`SixelPx`] each way. A Sixel can
/// describe more: `!65535~!1~` is 65536 columns, and a raster ratio of 20000
/// makes one band 120000 rows. The walk keeps counting past 65535, so an
/// extent too large to state is reported as such rather than stopped at the
/// limit while the painting runs on, and the caller refuses the image whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixelFit {
    /// Width and height are each at most 65535: the report's extent is exact.
    Fits,
    /// Wider or taller than 65535: the extent is stopped there and is not the
    /// image's.
    TooLarge,
}

/// The pixel aspect ratio a Sixel is drawn at, vertical to horizontal: a
/// sixel pixel is one device pixel wide and `num / den` device rows tall,
/// **preserved where it is not a whole number** by varying the rows each
/// sixel row takes.
///
/// `[[source: dec-std-070]]` 070-9 §7.1's table (PDF p. 918): `P1` omitted, 0,
/// 1 and 5 are 200:100, 2 is 450:100, 3 300:100, 4 250:100, 6 150:100, 7
/// 130:100, 8 112:100 and 9 100:100; a raster command `"Pan;Pad` before any
/// data overrides it ([`Sixel::decode`]). §12 lets a device fall back from a
/// grid it cannot image but says "The pixel aspect ratio should be preserved
/// if possible", and measures the actual grid as "the average grid size over
/// not more than 30 pixels" (PDF pp. 928-929): sixel row `y` starts at device
/// row `(y * num + den / 2) / den` from the image's top, rounded once, so
/// 4.5:1 takes rows of 5 and 4 in turn and every `P1` ratio repeats within 30.
/// The VT330/VT340 manual's column rounds each to a whole number, 5, 3, 2, 1
/// and 1 ("The VT300 rounds the pixel aspect ratio to the nearest integer"),
/// that terminal's own fallback; the ratio is preserved here (since
/// 2026-10-23).
///
/// **A ratio below one is drawn 1:1**: a row shorter than a device row is
/// "less than the smallest imageable unit of the device", which §12 then
/// images at that unit. **A `P1` past 9** is read as omitted, 2:1: 070-3
/// §3.5.1.3, "unimplemented selective parameter values of control sequences
/// shall be ignored as if that parameter were not received" (PDF p. 125),
/// which a DCS introducer is parsed as. Both were drawn 1:1, as was every
/// ratio that is not whole, until 2026-10-23.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SixelAspect {
    num: u16,
    den: u16,
}

impl SixelAspect {
    /// One device row a sixel row: what `P1 = 9` selects, and a raster
    /// `"1;1`.
    pub const ONE_TO_ONE: SixelAspect = SixelAspect { num: 1, den: 1 };

    /// 070-9 §7.1's ratio for macro values 0 to 9, over 100.
    const MACRO: [u16; 10] = [200, 200, 450, 300, 250, 200, 150, 130, 112, 100];

    /// The ratio the envelope's macro parameter `P1` sets; `None` when it is
    /// omitted, which both texts make 2:1, and a value past 9 as omitted.
    pub fn from_macro(p1: Option<u16>) -> SixelAspect {
        let v = p1.map_or(0, usize::from);
        let num = SixelAspect::MACRO
            .get(v)
            .copied()
            .unwrap_or(SixelAspect::MACRO[0]);
        SixelAspect { num, den: 100 }
    }

    /// The ratio a raster command's `Pan;Pad` sets. An omitted or zero
    /// parameter reads as one, 070-9 §8.1.2's guideline ("omitted parameters
    /// for Pn1 and Pn2 may be interpreted as '0' (zero) and default to '1'
    /// (one)", PDF p. 921), and a ratio below one is 1:1.
    fn from_raster(pan: u16, pad: u16) -> SixelAspect {
        let (num, den) = (pan.max(1), pad.max(1));
        match num < den {
            true => SixelAspect::ONE_TO_ONE,
            false => SixelAspect { num, den },
        }
    }

    /// The device row sixel row `y` starts at, from the image's top:
    /// `y` times the ratio, rounded half up once, so the rounding never
    /// accumulates and a body split anywhere lays its rows where the whole
    /// does.
    pub fn top(self, y: u32) -> u32 {
        let (num, den) = (u64::from(self.num), u64::from(self.den));
        let t = (u64::from(y) * num + den / 2) / den;
        u32::try_from(t).unwrap_or(u32::MAX)
    }

    /// The top of the band that holds device row `row`: the last band top
    /// at or above it. The estimate from the ratio is off by at most a band
    /// either way, which the two steps settle.
    fn band_top_at(self, row: u32) -> u32 {
        let (num, den) = (u64::from(self.num), u64::from(self.den));
        let mut n = u32::try_from(u64::from(row) * den / (6 * num)).unwrap_or(u32::MAX / 6);
        while n > 0 && self.top(n * 6) > row {
            n -= 1;
        }
        while self.top((n + 1).saturating_mul(6)) <= row {
            n += 1;
        }
        self.top(n * 6)
    }
}

/// **The device rows of one band's six sixel rows**: sixel row `k` of the
/// band covers rows `[tops[k], tops[k + 1])`, so `tops[0]` is the band's top
/// and `tops[6]` the next band's. Worked out when the band begins, from the
/// band's index and the ratio, so a data byte reads seven numbers and does
/// no arithmetic on the ratio.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Band {
    tops: [u32; 7],
}

impl Band {
    /// Band `n` (sixel rows `6n` to `6n + 5`) at `aspect`.
    fn of(n: u32, aspect: SixelAspect) -> Band {
        let first = n.saturating_mul(6);
        let mut tops = [0u32; 7];
        for (k, t) in tops.iter_mut().enumerate() {
            *t = aspect.top(first.saturating_add(k as u32));
        }
        Band { tops }
    }

    fn bottom(&self) -> u32 {
        self.tops[6]
    }
}

/// Whether a raster command may still set the ratio: only before the first
/// data byte and the first `$` or `-` (070-9 §8.1.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Raster {
    Open,
    Closed,
}

/// A caller-provided image the decoder paints into. `CLEAR` (zero) means a
/// pixel was never painted -- transparent, so a Sixel composited over the
/// screen shows the cells through its gaps -- which the reserved-value
/// discipline for blank cells is the precedent for.
pub struct SixelCanvas<'a> {
    pixels: &'a mut [Pixel],
    width: SixelPx,
    /// How many of the image's pixels each way one of the canvas's holds.
    by: u16,
}

impl<'a> SixelCanvas<'a> {
    /// A canvas `width` pixels wide over `pixels`; the height is what fits.
    ///
    /// A row of the image is `width` consecutive words.
    pub fn new(pixels: &'a mut [Pixel], width: SixelPx) -> SixelCanvas<'a> {
        SixelCanvas::shrunk(pixels, width, 1)
    }

    /// **A canvas holding the image at one `by`-th of its size each way**:
    /// pixel `(x, y)` lands at `(x / by, y / by)`, the last painted in each
    /// `by` by `by` block kept, so a line one pixel thin still shows. What
    /// a printer reads a picture larger than it holds into; a `by` of zero
    /// is one.
    pub fn shrunk(pixels: &'a mut [Pixel], width: SixelPx, by: u16) -> SixelCanvas<'a> {
        SixelCanvas {
            pixels,
            width,
            by: by.max(1),
        }
    }

    fn height(&self) -> SixelPx {
        match self.pixels.len().checked_div(self.width.index()) {
            Some(h) => SixelPx::of(u16::try_from(h).unwrap_or(u16::MAX)),
            None => SixelPx::ZERO,
        }
    }

    /// Paint one pixel. Returns whether it landed inside the canvas; a write
    /// past the edge is refused, never wrapped, and the caller counts it.
    fn paint(&mut self, x: SixelPx, y: SixelPx, colour: Pixel) -> Painted {
        let (x, y) = (
            SixelPx::of(x.raw() / self.by),
            SixelPx::of(y.raw() / self.by),
        );
        if x >= self.width || y >= self.height() {
            return Painted::Refused;
        }
        let i = y.index() * self.width.index() + x.index();
        match self.pixels.get_mut(i) {
            Some(slot) => {
                *slot = colour;
                Painted::Inside
            }
            None => Painted::Refused,
        }
    }
}

/// Whether a paint landed. A two-state answer rather than a `bool` so the call
/// site reads as what it is.
enum Painted {
    Inside,
    Refused,
}

/// What a decode produced. The used extent bounds what a renderer draws, and
/// the two counters are the "refuse and count" contract the design settles:
/// a Sixel is never drawn wrong, and how much it could not represent is a
/// number rather than a silent truncation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SixelReport {
    /// How wide the data reached, in pixels: the last column the cursor
    /// advanced through, **not** the rightmost painted pixel. A trailing `?`
    /// (all bits clear) still advances a column, and `[[source: ctlseqs]]`
    /// mode 8452 places the next text past the image's extent, so the extent
    /// is the data's, not the ink's. Independent of the canvas: it is what the
    /// sender described, and `refused` is what did not fit.
    pub width: SixelPx,
    /// How tall the data reached, in pixels: the bands a data byte occupied,
    /// each six sixel rows of as many pixels as the aspect ratio
    /// ([`SixelAspect`]). The extent, not the ink, for the reason `width` is.
    pub height: SixelPx,
    /// Pixels that would have fallen outside the canvas.
    pub refused: u32,
    /// Colour-register defines this decoder did not apply, so the caller knows
    /// the image is missing a colour: an unrecognised coordinate system, and
    /// an HLS (`Pu = 1`) define with a value past its range, which
    /// `[[source: dec-std-070]]` 070-9 §8.1.3 ignores whole. HLS itself is
    /// applied since 2026-10-11, its hue origin pinned (see `hls`); it was
    /// counted here until then.
    pub unapplied_registers: u32,
    /// Whether `width` and `height` are the image's or stopped at 65535.
    pub fit: SixelFit,
}

/// The 256 colour registers a Sixel image shares
/// (`[[source: vt3xx-sixel]]`, `Pc` 0..=255).
const REGISTERS: usize = 256;

/// **A register number standing in a claim's word** until the terminator
/// maps it to its colour ([`SixelStream::finish`]): [`Register::MARK`]
/// with the number in the low byte, so no mark is [`Pixel::CLEAR`], a
/// position the data left unset, or [`Pixel::FILL`], and a mark drawn by
/// mistake would draw as nothing, its alpha being zero.
struct Register;

impl Register {
    const MARK: u32 = 0x0000_0100;

    /// The mark of register `reg`, at most 255.
    #[inline]
    fn mark(reg: usize) -> Pixel {
        Pixel::from_bits(Register::MARK | u32::from(u8::try_from(reg).unwrap_or(u8::MAX)))
    }

    /// The register `mark` names.
    fn of(mark: Pixel) -> u8 {
        mark.bits().to_le_bytes()[0]
    }
}

/// **What the claim's words hold while the body arrives**: colours, as
/// long as each painted colour names one register, and register numbers
/// from the moment one could not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Words {
    /// Colours: a painted register's pixels hold its colour.
    Colour,
    /// Register numbers ([`Register::mark`]), which the terminator maps.
    Marks,
}

/// **The colour fast path**: a body's pixels are its registers' colours
/// as they are painted, as before recolouring was built, for as long as a
/// colour names exactly one register and none painted is redefined. Then
/// the terminator has nothing to map. The first data that would break it
/// -- a register painting in a colour clear or already a painted
/// register's, or a painted register redefined to another colour --
/// rewrites the words painted so far as their registers' numbers, once,
/// and the body goes on in numbers ([`Words::Marks`]), mapped at the
/// terminator. Every Sixel the corpus holds stays in colours.
///
/// **What a data byte paints is one load**, [`Inks::paint`] at its
/// register: the colour a painted register's pixels hold, its number once
/// the words are numbers, and [`Pixel::CLEAR`] for a register that has not
/// painted, whose first data asks whether its colour can stand for it
/// ([`Inks::first`]). Nothing is done per colour command.
#[derive(Clone)]
struct Inks {
    /// What the claim's words hold, colours or register numbers.
    words: Words,
    paint: [Pixel; REGISTERS],
    /// The painted colours, open addressing over [`Inks::SLOTS`] with
    /// [`Pixel::CLEAR`] free, and the register each names.
    seen: [Pixel; Inks::SLOTS],
    owner: [u8; Inks::SLOTS],
}

impl Inks {
    /// Twice the registers, so a probe is short.
    const SLOTS: usize = 2 * REGISTERS;

    /// Nothing painted, in colours.
    const NEW: Inks = Inks {
        words: Words::Colour,
        paint: [Pixel::CLEAR; REGISTERS],
        seen: [Pixel::CLEAR; Inks::SLOTS],
        owner: [0; Inks::SLOTS],
    };

    /// Where colour `c`'s probe starts: the multiplicative hash's high
    /// bits, as `StyleId::bucket` takes them.
    fn bucket(c: Pixel) -> usize {
        let h = c.bits().wrapping_mul(0x9E37_79B9) >> 23;
        usize::try_from(h).unwrap_or(0) % Inks::SLOTS
    }

    /// The slot colour `c` is in, or the free one its probe ends at.
    fn slot(&self, c: Pixel) -> usize {
        let mut i = Inks::bucket(c);
        while self.seen[i] != c && self.seen[i] != Pixel::CLEAR {
            i = (i + 1) % Inks::SLOTS;
        }
        i
    }

    /// **Register `r` paints for the first time, in `c`**: kept in colours
    /// when `c` is not clear and no painted register has it, and `false`
    /// when the body must go on in numbers.
    fn first(&mut self, r: usize, c: Pixel) -> bool {
        let i = self.slot(c);
        if c == Pixel::CLEAR || self.seen[i] == c {
            return false;
        }
        self.seen[i] = c;
        self.owner[i] = u8::try_from(r).unwrap_or(u8::MAX);
        self.paint[r] = c;
        true
    }

    /// The number of the register whose colour `c` is, `c` a painted one.
    fn register_of(&self, c: Pixel) -> u8 {
        self.owner[self.slot(c)]
    }

    /// Every register paints its number from here.
    fn numbers(&mut self) {
        self.words = Words::Marks;
        for (r, p) in self.paint.iter_mut().enumerate() {
            *p = Register::mark(r);
        }
    }
}

/// Which of the 256 registers a Sixel defined, a bit each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct RegisterSet([u64; REGISTERS / 64]);

impl RegisterSet {
    const NONE: RegisterSet = RegisterSet([0; REGISTERS / 64]);

    fn add(&mut self, r: usize) {
        self.0[r / 64] |= 1 << (r % 64);
    }

    fn has(self, r: usize) -> bool {
        self.0[r / 64] & (1 << (r % 64)) != 0
    }

    fn with(self, other: RegisterSet) -> RegisterSet {
        let mut out = self;
        for (o, b) in out.0.iter_mut().zip(other.0) {
            *o |= b;
        }
        out
    }
}

/// Whether a Sixel's colour registers are its own or carried over from the
/// Sixels before it: `?1070`, "use private color registers for each
/// graphic" (`[[source: ctlseqs]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixelRegisters {
    /// `?1070` set, the default: every register starts undefined.
    Private,
    /// `?1070` reset: a register a shared Sixel defined starts the next
    /// shared Sixel with that definition, and a shared picture already
    /// drawn with a register a later shared Sixel redefines takes the new
    /// colour, as a VT340's one colour map and xterm's shared set make it:
    /// such a picture keeps the register each pixel was drawn with, a byte
    /// a pixel ([`SixelStream::finish`]).
    Shared,
}

/// How far down, or right, a Sixel may go: display mode's edge of the
/// screen, or no limit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixelEdge {
    /// Sixel scrolling: the image is as tall and as wide as its data.
    Open,
    /// Display mode on a screen `px` device pixels tall, or wide: at the
    /// bottom no graphics new line starts a band at or past it, and rows
    /// past it are not kept; at the right no column at or past it is
    /// painted or counted in the width.
    At(u32),
}

impl SixelEdge {
    /// The edge of a screen `cells` cells of `cell` pixels each, or no limit
    /// while the cell size is unknown (zero), since nothing then says where
    /// the edge is.
    pub fn screen(cells: usize, cell: ImagePx) -> SixelEdge {
        if cell == ImagePx::ZERO {
            return SixelEdge::Open;
        }
        let px = u64::from(cell.raw()).saturating_mul(cells as u64);
        SixelEdge::At(u32::try_from(px).unwrap_or(u32::MAX))
    }

    fn px(self) -> u32 {
        match self {
            SixelEdge::Open => u32::MAX,
            SixelEdge::At(px) => px,
        }
    }
}

/// **What a Sixel does with the positions its data leaves at zero**, its
/// `P2`: `[[source: vt3xx-sixel]]`, 1 "Pixel positions specified as 0
/// remain at their current color", 0 or 2 "Pixel positions specified as 0
/// are set to the current background color" (lines 192-203).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixelFill {
    /// `P2` = 1: they stay clear, and what is under the image shows.
    Keep,
    /// `P2` = 0 or 2: they are set to the background ([`Pixel::FILL`]) over
    /// the raster extent `Ph` by `Pv` when a raster command sends one -- "The
    /// VT300 uses Ph and Pv to erase the background when P2 is set to 0 or 2"
    /// (`[[source: vt3xx-sixel]]` 302-303) -- and otherwise over `w` by `h`
    /// device pixels from the image's corner, the extent
    /// `[[source: dec-std-070]]` 070-9 §10 starts a Sixel with, "Distance
    /// from current ANSI text position to current ANSI right margin" and "to
    /// current ANSI bottom margin" (PDF p. 926). Zero where the terminal does
    /// not know its cell size, and the fill then covers the data's reach on
    /// that side.
    ///
    /// `h` is also the rows left to the bottom margin, past which the image
    /// scrolls the screen and the fill goes on, in whole cells of `cell`
    /// device pixels ([`SixelStream::finish`]); `cell` is zero where the
    /// terminal does not know it.
    Background { w: u32, h: u32, cell: u32 },
}

/// How a terminal begins one Sixel: its aspect ratio, the colour an
/// undefined register paints with, whose registers it starts from, how
/// far down and right it may go, and what it does with its zero positions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SixelBegin {
    pub aspect: SixelAspect,
    /// `[[source: dec-std-070]]` 070-9 §10, "Sixel color: Contrasting to
    /// background. Color numbers: All unspecified color numbers image as the
    /// default Sixel color": every register the Sixel has not defined paints
    /// with this, `Pixel::CLEAR` -- drawn as nothing -- while the
    /// terminal does not know its foreground.
    pub ink: Pixel,
    pub registers: SixelRegisters,
    pub floor: SixelEdge,
    /// `[[source: dec-std-070]]` 070-9 §11.2.2, "Right Margin: Sixels
    /// defined to be printed past the right margin are not printed".
    pub right: SixelEdge,
    pub fill: SixelFill,
}

/// The Sixel decoder over a body held whole: stateless between calls, each a
/// pass of the one grammar over the data with the caller's canvas. A terminal
/// does not hold the body, and decodes it with [`SixelStream`].
pub struct Sixel;

impl Sixel {
    /// The byte a Sixel's handler stores for body byte `b` under `enc`, or
    /// `None` when the byte is to be dropped before the decoder.
    ///
    /// `[[source: dec-std-070]]` 070-9 §9 (PDF pp. 925-926), read for a body:
    /// the format effectors "shall have no effect on the interpretation of
    /// Sixel Control strings" and every other C0 that reaches a handler is
    /// ignored (§9.1); "2/0 (SPACE) Ignored ... 7/15 (DELETE) Ignored" (§9.2);
    /// and GR codes "are errors. The 8th bit is set to zero and the code is
    /// then processed as a GL code" (§9.4), so under ISO 8859-1 `A0` is a
    /// `SPACE` and `FF` a `DEL`, both dropped. §8.1 (PDF p. 919) says what
    /// the drop is for: a command "is terminated by any non parameter code
    /// ... Note Format Effectors (0/8 to 0/13) and SPACE (2/0) are ignored
    /// within sixel control strings", so neither ends a number. Under UTF-8
    /// `80-FF` belong to a character and are kept as they came, and under ISO
    /// 8859-1 `80-9F` are C1 controls the graph takes before a body sees them
    /// (§9.3), so both are kept. `[[source: williams]]` treats GR as GL "in
    /// all states" (462), and `[[source: ecma48]]` §9 gives "the contents of
    /// the command string" the GL meanings, so the texts agree.
    ///
    /// **The common byte is decided by one compare**
    /// ([`Sixel::body_byte_is_gl_past_space`]), and the encoding is read only
    /// for `A0-FF`. A fold and two compares on every byte, the first shape,
    /// retired 16.5% more instructions on `sixel/yazi-walk.raw`.
    #[inline]
    pub fn body_byte(enc: InputEncoding, b: StreamByte) -> Option<StreamByte> {
        match b.raw_byte() {
            0x21..=0x7e => Some(b),
            _ => Sixel::body_byte_rare(enc, b),
        }
    }

    /// Whether `b` is in `21-7F`, GL past `SPACE`. [`Sixel::body_byte`] keeps
    /// every byte here but `DEL` as it is, under both encodings: every Sixel
    /// command and data byte. A handler behind baud's graph never sees a
    /// `DEL`, which the `DcsStr` row ignores as `[[source: williams]]`'s
    /// `dcs passthrough` does (388), so on its per-byte path it tests this
    /// inline, stores the byte as it is, and sends the rest to `body_byte`
    /// out of line.
    ///
    /// **One signed compare, the cost of the `is_c0` it replaced**: as an
    /// `i8`, `21-7F` are the bytes above `0x20`. Written as a `match` on `u8`
    /// ranges with `80-9F` beside them, LLVM tested the high bit first and the
    /// common byte took seven instructions where `is_c0` took three; as an
    /// unsigned range `21-7E` it took four, a subtract, a mask, a compare and
    /// a branch.
    #[inline]
    pub fn body_byte_is_gl_past_space(b: StreamByte) -> bool {
        i8::from_ne_bytes([b.raw_byte()]) > 0x20
    }

    /// [`Sixel::body_byte`] for a byte outside `21-7E`: a C0, `SPACE`, `DEL`,
    /// or one in `80-FF`, none of which a Sixel encoder sends but a line
    /// break.
    #[cold]
    #[inline(never)]
    fn body_byte_rare(enc: InputEncoding, b: StreamByte) -> Option<StreamByte> {
        match b.raw_byte() {
            0x80..=0x9f => Some(b),
            0xa0..=0xff => {
                let gl = enc.gr_as_gl(b);
                match gl.raw_byte() {
                    0x20 | 0x7f => None,
                    _ => Some(gl),
                }
            }
            _ => None,
        }
    }

    /// Decode `data` -- the bytes after the `q` and before the `ST` -- into
    /// `canvas`. The canvas starts however the caller cleared it (`CLEAR` for
    /// transparent), and set data bits paint the current register; unset bits
    /// leave the pixel, so a later Sixel over the same canvas overlays it.
    ///
    /// `aspect` is what the envelope's `P1` set ([`SixelAspect::from_macro`]);
    /// a raster command at the head of `data` overrides it. A sixel row is
    /// painted that many device rows tall, and a band is six of them.
    ///
    /// **The bytes go through [`SixelStream`]'s grammar one at a time**, the
    /// one walk the terminal decodes a body with as it arrives; only where the
    /// pixels land differs, a canvas the caller sized here and the store's
    /// arena there, and when a pixel takes its colour: here as it is
    /// painted, as a printer inks its paper, and there when the body ends,
    /// as a screen's colour map shows a redefinition over what was drawn
    /// before it.
    pub fn decode(data: &[u8], aspect: SixelAspect, canvas: &mut SixelCanvas) -> SixelReport {
        Sixel::decode_inked(data, aspect, Pixel::CLEAR, canvas)
    }

    /// [`Sixel::decode`] with an undefined register painting `ink`: what a
    /// printer reading a dump does, whose "default Sixel color" is its
    /// paper's contrast (070-9 § 10) and whose monochrome dump names no
    /// register at all.
    pub fn decode_inked(
        data: &[u8],
        aspect: SixelAspect,
        ink: Pixel,
        canvas: &mut SixelCanvas,
    ) -> SixelReport {
        let mut walk = Walk::new(aspect);
        walk.regs = [ink; REGISTERS];
        let mut sink = Clipped { canvas, refused: 0 };
        for &b in data {
            walk.put(b, &mut sink);
        }
        walk.close(&mut sink);
        walk.report(sink.refused)
    }

    /// The extent the data reaches -- its `width` and `height` -- without
    /// painting. **The same walk as [`Sixel::decode`], not a second grammar**,
    /// so a size reserved from `extent` is exactly what `decode` fills; two
    /// spellings of one grammar is the `csi_whole` mistake this repo already
    /// paid for.
    pub fn extent(data: &[u8], aspect: SixelAspect) -> SixelReport {
        let mut walk = Walk::new(aspect);
        for &b in data {
            walk.put(b, &mut Measure);
        }
        walk.close(&mut Measure);
        walk.report(0)
    }
}

/// The command whose parameters are arriving: none, a colour introducer
/// `#`, a repeat introducer `!`, or raster attributes `"`. A Sixel's
/// parameters are numbers separated by `;`, so a body split anywhere --
/// inside a number included -- leaves this and the number so far, and the
/// next read goes on from them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cmd {
    Ground,
    Colour,
    Repeat,
    Raster,
}

impl Cmd {
    /// The index of the last parameter the command takes: `#Pc;Pu;Px;Py;Pz`
    /// five, `"Pan;Pad;Ph;Pv` four, `!Pn` one. A `;` past it ends the
    /// command, and is then a byte of no meaning.
    fn last(self) -> usize {
        match self {
            Cmd::Colour => 4,
            Cmd::Raster => 3,
            Cmd::Repeat | Cmd::Ground => 0,
        }
    }
}

/// Where a walk's pixels go. The grammar is one ([`Walk`]); what a set bit
/// does is the sink's: nothing ([`Measure`]), a pixel in a canvas the caller
/// sized ([`Clipped`]), or a pixel in the gap of the image store's arena a
/// Sixel claimed as it began ([`Claim`]).
trait Sink {
    /// The raster's extent when one is set before any data: `Ph`, and `Pv`
    /// in device rows -- "the vertical by the aspect ratio"
    /// (`[[source: vt340test-physicalsixels]]` 193-195).
    fn extent(&mut self, ph: u16, pv: u32);

    /// One data byte, `bits` its six bits, drawn over columns `[x0, x1)` --
    /// one, or a repeat count's -- in `band`, with register `reg`, whose
    /// colour is `colour` now. Called for every data byte, all bits clear
    /// included, because the image reaches that far.
    fn data(&mut self, x0: u32, x1: u32, band: &Band, bits: u8, reg: usize, colour: Pixel);

    /// The device row no graphics new line may start a band at or past:
    /// display mode's bottom edge ([`SixelEdge`]), else none.
    fn floor(&self) -> u32 {
        u32::MAX
    }

    /// Register `r` was defined as `colour`, which a shared set keeps
    /// ([`SixelRegisters::Shared`]).
    fn defined(&mut self, _r: usize, _colour: Pixel) {}
}

/// The sink that paints nothing: what [`Sixel::extent`] measures with.
struct Measure;

impl Sink for Measure {
    fn extent(&mut self, _ph: u16, _pv: u32) {}
    fn data(&mut self, _x0: u32, _x1: u32, _band: &Band, _bits: u8, _r: usize, _c: Pixel) {}
}

/// The sink over a caller's canvas: a pixel past its edge is refused and
/// counted, never wrapped.
struct Clipped<'c, 'a> {
    canvas: &'c mut SixelCanvas<'a>,
    refused: u32,
}

impl Sink for Clipped<'_, '_> {
    fn extent(&mut self, _ph: u16, _pv: u32) {}

    fn data(&mut self, x0: u32, x1: u32, band: &Band, bits: u8, _reg: usize, colour: Pixel) {
        for x in x0..x1 {
            for row in 0..6 {
                if bits & (1 << row) == 0 {
                    continue;
                }
                for y in band.tops[row]..band.tops[row + 1] {
                    let painted = match (SixelPx::at(x), SixelPx::at(y)) {
                        (Some(px), Some(py)) => self.canvas.paint(px, py, colour),
                        _ => Painted::Refused,
                    };
                    if let Painted::Refused = painted {
                        self.refused = self.refused.saturating_add(1);
                    }
                }
            }
        }
    }
}

/// The grammar of a Sixel body, one byte at a time: the colour registers,
/// the active position, the extent reached, and the command and number in
/// progress. What a body leaves between two reads is this, so a split
/// anywhere decodes as the whole does.
///
/// `[[source: vt3xx-sixel]]` gives the grammar and `[[source: dec-std-070]]`
/// 070-9 §8 the rules it leaves open; each arm below cites the one it keeps.
#[derive(Clone)]
struct Walk {
    regs: [Pixel; REGISTERS],
    /// The pixel aspect ratio, whether a raster command may still change
    /// it, and the band the active position is in, by index and in device
    /// rows.
    aspect: SixelAspect,
    raster: Raster,
    band_n: u32,
    band: Band,
    reg: usize,
    /// Positions and the extent, counted in `u32` past what a coordinate
    /// holds, so an image too large to state is seen as one.
    x: u32,
    width: u32,
    height: u32,
    /// The repeat introducer applies to the one data byte that follows it;
    /// one column otherwise.
    repeat: u16,
    unapplied: u32,
    cmd: Cmd,
    /// The parameter being read, and the ones read before it.
    stage: usize,
    val: u16,
    params: [u16; 5],
}

impl Walk {
    const NEW: Walk = Walk {
        regs: [Pixel::CLEAR; REGISTERS],
        aspect: SixelAspect { num: 200, den: 100 },
        raster: Raster::Open,
        band_n: 0,
        band: Band {
            tops: [0, 2, 4, 6, 8, 10, 12],
        },
        reg: 0,
        x: 0,
        width: 0,
        height: 0,
        repeat: 1,
        unapplied: 0,
        cmd: Cmd::Ground,
        stage: 0,
        val: 0,
        params: [0; 5],
    };

    fn new(aspect: SixelAspect) -> Walk {
        Walk {
            aspect,
            band: Band::of(0, aspect),
            ..Walk::NEW
        }
    }

    /// One body byte. A digit grows the number in progress, saturating at
    /// `u16::MAX`; any other byte ends it, and a byte that ends a command is
    /// then read as the command's successor, as the slice walk this replaced
    /// read the byte after a number.
    #[inline]
    fn put(&mut self, b: u8, sink: &mut impl Sink) {
        if self.cmd == Cmd::Ground {
            return self.ground(b, sink);
        }
        if b.is_ascii_digit() {
            self.val = self
                .val
                .saturating_mul(10)
                .saturating_add(u16::from(b - b'0'));
            return;
        }
        self.params[self.stage] = self.val;
        self.val = 0;
        if b == b';' && self.stage < self.cmd.last() {
            self.stage += 1;
            return;
        }
        self.apply(sink);
        self.ground(b, sink);
    }

    /// The body ended: a command whose parameters ran to the terminator takes
    /// effect with what arrived, as the slice walk read a number at the end
    /// of its data.
    fn close(&mut self, sink: &mut impl Sink) {
        if self.cmd != Cmd::Ground {
            self.params[self.stage] = self.val;
            self.val = 0;
            self.apply(sink);
        }
    }

    fn begin(&mut self, cmd: Cmd) {
        self.cmd = cmd;
        self.stage = 0;
        self.val = 0;
        self.params = [0; 5];
    }

    /// The command in progress, its parameters read; one never given is
    /// zero, which is the default a Sixel omits.
    fn apply(&mut self, sink: &mut impl Sink) {
        let p = self.params;
        match self.cmd {
            // Colour register: `#Pc` selects, `#Pc;Pu;Px;Py;Pz` defines.
            Cmd::Colour => {
                let named = usize::from(p[0]).min(REGISTERS - 1);
                if self.stage == 0 {
                    self.reg = named;
                } else {
                    match p[1] {
                        2 => {
                            self.regs[named] = rgb(p[2], p[3], p[4]);
                            sink.defined(named, self.regs[named]);
                            self.reg = named;
                        }
                        // HLS, hue 0-360 and lightness and saturation 0-100
                        // (`[[source: vt3xx-sixel]]` Table 14-1). A value past
                        // its range ignores the whole command, `Pc` included:
                        // `[[source: dec-std-070]]` 070-9 §8.1.3 (PDF p. 921),
                        // so `reg` is kept.
                        1 => match hls(p[2], p[3], p[4]) {
                            Some(c) => {
                                self.regs[named] = c;
                                sink.defined(named, c);
                                self.reg = named;
                            }
                            None => self.unapplied = self.unapplied.saturating_add(1),
                        },
                        // An unrecognised system selects `Pc` and is counted;
                        // 070-9 §8.1.3 ignores this whole command too, and RGB
                        // past 100 is clamped where it says the same: note
                        // AN's item 6, not built here.
                        _ => {
                            self.reg = named;
                            self.unapplied = self.unapplied.saturating_add(1);
                        }
                    }
                }
                self.repeat = 1;
            }
            // Repeat introducer: the next data byte is drawn `Pn` times.
            // `[[source: vt3xx-sixel]]` is silent on `Pn = 0`;
            // `[[source: dec-std-070]]` 070-9 §8.1.1 settles it: "A repeat
            // count of zero implies a repeat count of 1" (PDF p. 920).
            Cmd::Repeat => self.repeat = p[0].max(1),
            // Raster attributes: the aspect ratio `Pan:Pad`, which sets the
            // rows a sixel row takes, and a horizontal extent `Ph`, which
            // 070-9 §8.1.2 gives "to provide a summary to avoid a complete
            // scanning of the data for the horizontal limits" and which "does
            // not limit the number of pixels a device may image" (its p. 9,
            // PDF p. 920): the sink may lay its rows out by it, and data past
            // it is drawn. `Pv`, "the same functionality vertically" (p. 10),
            // changes nothing a sink does: a claim is the whole gap. After
            // data or a `$` or `-` the whole command is ignored, as §8.1.2
            // says.
            Cmd::Raster => {
                if let Raster::Open = self.raster {
                    self.aspect = SixelAspect::from_raster(p[0], p[1]);
                    self.band = Band::of(self.band_n, self.aspect);
                    sink.extent(p[2], self.aspect.top(u32::from(p[3])));
                }
                self.repeat = 1;
            }
            Cmd::Ground => {}
        }
        self.cmd = Cmd::Ground;
        self.stage = 0;
    }

    #[inline]
    fn ground(&mut self, b: u8, sink: &mut impl Sink) {
        match b {
            b'#' => self.begin(Cmd::Colour),
            b'!' => self.begin(Cmd::Repeat),
            b'"' => self.begin(Cmd::Raster),
            // Graphics carriage return: back to the left of this band.
            b'$' => {
                self.x = 0;
                self.repeat = 1;
                self.raster = Raster::Closed;
            }
            // Graphics new line: left, and down one six-pixel band -- unless
            // the band would start at or past the floor, display mode's
            // bottom edge: `[[source: vt3xx-sixel]]`, "The terminal ignores
            // any commands that attempt to advance the active position below
            // the bottom margin of the graphics page", so the command moves
            // nothing and later data continues along the last band. With no
            // floor the comparison never holds.
            b'-' => {
                if self.band.bottom() < sink.floor() {
                    self.x = 0;
                    self.band_n = self.band_n.saturating_add(1);
                    self.band = Band::of(self.band_n, self.aspect);
                }
                self.repeat = 1;
                self.raster = Raster::Closed;
            }
            // A sixel data byte: six vertical pixels, the low six bits of
            // `b - 0x3F`, least significant at the top
            // (`[[source: vt3xx-sixel]]`).
            0x3f..=0x7e => {
                let x1 = self.x.saturating_add(u32::from(self.repeat));
                let colour = self.regs[self.reg];
                sink.data(self.x, x1, &self.band, b - 0x3f, self.reg, colour);
                // The extent is where the data reached, not where pixels were
                // set: `x1` is the first column past this data byte, and the
                // band is six rows tall whether or not any bit set.
                self.x = x1;
                self.width = self.width.max(x1);
                self.height = self.height.max(self.band.bottom());
                self.repeat = 1;
                self.raster = Raster::Closed;
            }
            // Everything else is skipped. No C0, `SPACE` or `DEL` reaches here
            // from the core -- the Sixel arm of `str_put` keeps only what
            // [`Sixel::body_byte`] keeps, a GR byte under ISO 8859-1 as its GL
            // byte -- so what does is a byte in `80-FF` under UTF-8, a digit
            // or `;` outside a command, and a command this decoder does not
            // recognise (note AN, section 11). A caller that hands this a body
            // with a `SPACE` in a number gets the number ended there: the drop
            // is `body_byte`'s, before the byte arrives.
            _ => {}
        }
    }

    fn report(&self, refused: u32) -> SixelReport {
        let fit = match (SixelPx::at(self.width), SixelPx::at(self.height)) {
            (Some(_), Some(_)) => SixelFit::Fits,
            _ => SixelFit::TooLarge,
        };
        SixelReport {
            width: SixelPx::at(self.width).unwrap_or(SixelPx(u16::MAX)),
            height: SixelPx::at(self.height).unwrap_or(SixelPx(u16::MAX)),
            refused,
            unapplied_registers: self.unapplied,
            fit,
        }
    }
}

/// Whether the gap a Sixel claimed still holds what it has painted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    /// Every data byte so far fitted.
    Yes,
    /// One did not: wider or taller than a coordinate holds, or more pixels
    /// than the gap. The rest of the body is walked without painting, so the
    /// refusal at the terminator names the image's whole size.
    Lost,
}

/// The gap of the image store's arena a Sixel paints into while its body
/// arrives, and how its rows are laid out there.
///
/// **Row `r` of the image so far is the `stride` words at `r * stride`**,
/// and rows `[0, zeroed)` are cleared or painted -- the rest of the gap is
/// whatever the arena held, an earlier image's pixels included, and is
/// cleared as the image grows into it. The stride starts at the raster's
/// `Ph` when one is sent, the summary 070-9 §8.1.2 gives it as; with none,
/// or past it, it grows by doubling, and a band below the first lays the
/// rows out again at the widest row seen when the stride is wider, so an
/// overstated `Ph` or a doubling commits no more rows of it than the image
/// has. At the terminator the rows are packed to the image's width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Gap {
    /// Where the claim starts in the arena, and how many words it holds.
    at: usize,
    len: usize,
    stride: usize,
    zeroed: usize,
    /// The widest row painted so far, in pixels.
    widest: usize,
    /// One past the highest word of the claim written, for
    /// [`SixelStream::written`].
    written: usize,
    held: Held,
    /// The Sixel's floor ([`Sink::floor`]), its right edge, and the
    /// registers it defined ([`Sink::defined`]): kept here, with the claim,
    /// so the walk's own layout is the one its byte loop was measured with.
    /// **`stride` never passes `right`**, so a data byte that reaches past
    /// the edge always takes [`Claim::reach`]'s slow path, which clips it,
    /// and the fast path is the one without an edge.
    floor: u32,
    right: u32,
    defined: RegisterSet,
    /// The raster extent a command before any data sent, zero where it
    /// sent none: the region a background fill covers ([`SixelFill`]).
    raster: (u32, u32),
}

impl Gap {
    const NONE: Gap = Gap {
        at: 0,
        len: 0,
        stride: 0,
        zeroed: 0,
        widest: 0,
        written: 0,
        held: Held::Yes,
        floor: u32::MAX,
        right: u32::MAX,
        defined: RegisterSet::NONE,
        raster: (0, 0),
    };
}

/// The sink a terminal decodes into: the claimed gap of the store's arena,
/// and what its words hold ([`Inks`]).
struct Claim<'g, 's, 'b> {
    st: &'g mut ClaimState,
    store: &'s mut ImageStore<'b>,
}

/// **A claim's gap and what its words hold, behind one reference**, so a
/// [`Claim`] is two pointers, which a call passes in registers, as it was
/// before the words could be colours or numbers: a third went through
/// memory on every byte. **The gap first, in declared order**, so its
/// fields sit at the small offsets a paired load reaches: with the 3.5 KB
/// of tables before it, the byte loop's loads of the gap split in two.
#[derive(Clone)]
#[repr(C)]
struct ClaimState {
    gap: Gap,
    inks: Inks,
}

impl ClaimState {
    const NEW: ClaimState = ClaimState {
        gap: Gap::NONE,
        inks: Inks::NEW,
    };
}

impl Claim<'_, '_, '_> {
    /// The claim's words, from its start.
    fn words(&mut self) -> &mut [Pixel] {
        let Gap { at, len, .. } = self.st.gap;
        &mut self.store.arena_mut()[at..at + len]
    }

    /// The image needs `need` words and the claim holds fewer: a free since
    /// the `q` -- a resize expiring an image between two reads of the
    /// body -- may have opened a larger gap, so take the store's largest
    /// gap now and move the rows laid out so far into it. Whether the claim
    /// now holds `need`.
    fn regrow(&mut self, need: usize) -> bool {
        let (at, len) = self.store.largest_gap();
        if len < need {
            return false;
        }
        let g = self.st.gap;
        let used = g.zeroed * g.stride;
        self.store.arena_mut().copy_within(g.at..g.at + used, at);
        self.st.gap.at = at;
        self.st.gap.len = len;
        self.st.gap.written = used;
        moved(self.store, &g, at);
        true
    }

    /// Make the image reach the columns `[x0, x1)` and `h` rows inside the
    /// claim, clearing what it grows into, and say which of those columns
    /// to paint: all of them, or in display mode none at or past the right
    /// edge. A byte wholly past it still lays its band's rows out, since
    /// the band counts in the height. [`Held::Lost`] when the claim cannot.
    fn reach(&mut self, x0: u32, x1: u32, h: u32) -> (u32, u32) {
        let g = self.st.gap;
        // Inside the stride, which is inside the right edge: nothing to clip.
        if x1 as usize <= g.stride && h as usize <= g.zeroed {
            self.st.gap.widest = g.widest.max(x1 as usize);
            return (x0, x1);
        }
        let (x0, x1) = (x0.min(g.right), x1.min(g.right));
        self.grow(x1, h);
        (x0, x1)
    }

    /// [`Claim::reach`] past the rows or the stride laid out.
    fn grow(&mut self, w: u32, h: u32) {
        if w > u32::from(u16::MAX) || h > u32::from(u16::MAX) {
            self.st.gap.held = Held::Lost;
            return;
        }
        let (w, h) = (w as usize, h as usize);
        let g = self.st.gap;
        if w <= g.stride && h <= g.zeroed {
            self.st.gap.widest = g.widest.max(w);
            return;
        }
        // Past the rows laid out: the image is at least the widest row by
        // the rows reached, and a claim smaller than that is grown or lost.
        let need = w.max(g.widest).saturating_mul(h.max(g.zeroed));
        if need > g.len && !self.regrow(need) {
            self.st.gap.held = Held::Lost;
            return;
        }
        let g = self.st.gap;
        let rows = h.max(g.zeroed);
        if w > g.stride {
            // Wider than the stride: double it, or take exactly `w` when a
            // doubled stride would not leave the claim room for the rows,
            // never past the right edge (`w` is not).
            let grown = w.max(g.stride * 2).max(64).min(g.right as usize);
            let fits = g.len.checked_div(rows).unwrap_or(0);
            let to = if grown <= fits { grown } else { w };
            if to.saturating_mul(rows) > g.len {
                self.st.gap.held = Held::Lost;
                return;
            }
            self.restride(to);
        }
        self.st.gap.widest = self.st.gap.widest.max(w);
        if h > self.st.gap.zeroed {
            // A new band: a stride past the widest row is laid out again at
            // it, so the rows below take only what the image is wide.
            let widest = self.st.gap.widest;
            if self.st.gap.zeroed > 0 && self.st.gap.stride > widest {
                self.restride(widest);
            }
            if h.saturating_mul(self.st.gap.stride) > g.len {
                if widest.saturating_mul(h) > g.len {
                    self.st.gap.held = Held::Lost;
                    return;
                }
                self.restride(widest);
            }
            let (from, to) = (
                self.st.gap.zeroed * self.st.gap.stride,
                h * self.st.gap.stride,
            );
            self.words()[from..to].fill(Pixel::CLEAR);
            self.wrote(to);
            self.st.gap.zeroed = h;
        }
    }

    /// Lay the cleared rows out at stride `to`, clearing the columns a
    /// wider stride adds. Rows move in the order that never overwrites one
    /// not yet moved: from the last when widening, from the first when
    /// narrowing.
    fn restride(&mut self, to: usize) {
        let Gap {
            stride: from,
            zeroed,
            ..
        } = self.st.gap;
        if to == from {
            return;
        }
        let keep = from.min(to);
        let words = self.words();
        if to > from {
            for r in (0..zeroed).rev() {
                words.copy_within(r * from..r * from + keep, r * to);
                words[r * to + from..r * to + to].fill(Pixel::CLEAR);
            }
        } else {
            for r in 0..zeroed {
                words.copy_within(r * from..r * from + keep, r * to);
            }
        }
        self.wrote(zeroed * to);
        self.st.gap.stride = to;
    }

    /// **Register `reg`'s first data, in `colour`**: the word it paints,
    /// its colour while that still names it alone, else its number once
    /// the words painted so far are numbers too ([`Claim::switch_to_numbers`]).
    #[cold]
    #[inline(never)]
    fn first(&mut self, reg: usize, colour: Pixel) -> Pixel {
        if self.st.inks.first(reg, colour) {
            return colour;
        }
        self.switch_to_numbers();
        Register::mark(reg)
    }

    /// **Rewrite every word painted so far as its register's number**, and
    /// paint numbers from here: each colour painted names one register,
    /// which [`Inks`] keeps, so the rewrite is exact. Once a body at most.
    #[cold]
    #[inline(never)]
    fn switch_to_numbers(&mut self) {
        if self.st.inks.words == Words::Marks {
            return;
        }
        if self.st.gap.held != Held::Lost {
            self.rewrite_as_numbers();
        }
        self.st.inks.numbers();
    }

    /// [`Claim::switch_to_numbers`]'s rewrite of the words painted so far.
    fn rewrite_as_numbers(&mut self) {
        let Gap {
            at, stride, zeroed, ..
        } = self.st.gap;
        let inks = &self.st.inks;
        let (mut last, mut its) = (Pixel::CLEAR, Pixel::CLEAR);
        let used = stride * zeroed;
        for w in &mut self.store.arena_mut()[at..at + used] {
            if *w == Pixel::CLEAR {
                continue;
            }
            if *w != last {
                last = *w;
                its = Register::mark(usize::from(inks.register_of(*w)));
            }
            *w = its;
        }
    }

    /// The claim has written its words up to `to`: the store keeps the
    /// pages they touch, and only those, from a release beside it.
    fn wrote(&mut self, to: usize) {
        self.st.gap.written = self.st.gap.written.max(to);
        let Gap { at, written, .. } = self.st.gap;
        self.store.set_claim(Some((at, at + written)));
    }
}

/// **A claim moved to a larger gap gives back what it left**: the old
/// claim's written words, but the pages the rows now at `at` touch. Out of
/// line, off the byte loop: a claim moves at most once a free during its
/// body.
#[cold]
#[inline(never)]
fn moved(store: &mut ImageStore, old: &Gap, at: usize) {
    let used = old.zeroed * old.stride;
    store.set_claim(Some((at, at + used)));
    store.release_free(old.at, old.at + old.written);
}

impl Sink for Claim<'_, '_, '_> {
    fn extent(&mut self, ph: u16, pv: u32) {
        // Only before any data, so no row is laid out yet to move; never
        // past the right edge.
        if ph > 0 {
            self.st.gap.stride = usize::from(ph).min(self.st.gap.right as usize);
        }
        self.st.gap.raster = (u32::from(ph), pv);
    }

    fn data(&mut self, x0: u32, x1: u32, band: &Band, bits: u8, reg: usize, colour: Pixel) {
        if self.st.gap.held == Held::Lost {
            return;
        }
        let (x0, x1) = self.reach(x0, x1, band.bottom());
        if self.st.gap.held == Held::Lost || bits == 0 {
            return;
        }
        // Inside the claim by `reach`: `x0..x1` is at most the stride, empty
        // past the right edge, and the band's last row is below `zeroed`.
        // **The colour while it names the register** ([`Inks`]), else the
        // register: nothing draws the claim before the terminator, which
        // maps each number to the colour its register ends with
        // ([`SixelStream::finish`]).
        let mut mark = self.st.inks.paint[reg];
        if mark == Pixel::CLEAR {
            mark = self.first(reg, colour);
        }
        let stride = self.st.gap.stride;
        let (x0, x1) = (x0 as usize, x1 as usize);
        let words = self.words();
        for row in 0..6 {
            if bits & (1 << row) == 0 {
                continue;
            }
            for y in band.tops[row] as usize..band.tops[row + 1] as usize {
                words[y * stride + x0..y * stride + x1].fill(mark);
            }
        }
    }

    fn floor(&self) -> u32 {
        self.st.gap.floor
    }

    fn defined(&mut self, r: usize, colour: Pixel) {
        self.st.gap.defined.add(r);
        // A painted register's pixels hold its colour while the words are
        // colours: a new one is a redefinition they must follow.
        let painted = self.st.inks.paint[r];
        if self.st.inks.words == Words::Colour && painted != Pixel::CLEAR && painted != colour {
            self.switch_to_numbers();
        }
    }
}

/// A Sixel decoded **as its body arrives**, into the image store's arena,
/// with no copy of the body kept. One per terminal: [`SixelStream::begin`]
/// at the `q`, [`SixelStream::put`] for each body byte the handler keeps,
/// [`SixelStream::finish`] at the terminator.
///
/// **What limits an image is its pixels**, as `[[source: dec-std-070]]`
/// 070-9 has it: §8.1.2's extent "does not limit the number of pixels a
/// device may image" (its p. 9, PDF p. 920), §2 sets as a goal that
/// "'picture too complex' for the above raster sizes is not permitted" --
/// 768 or 800 by 240 pixels at 2:1 -- and "The protocol should support any
/// size rasters, although not all devices will be capable of displaying
/// them" (p. 2, PDF p. 913), and no section read names a limit on a body's
/// bytes. So an image
/// is refused for what it would occupy -- wider or taller than 65535, more
/// pixels than the arena's largest gap, or no free slot -- and never for how
/// many bytes described it.
///
/// **At the `q` it claims the store's largest gap.** Nothing else stores an
/// image until this one ends; a free between two reads (a resize expiring an
/// image) can open a larger gap, and a claim that runs out takes the store's
/// largest gap then, moving its rows; a claim is not recorded, so a body
/// that never reaches its terminator -- cancelled, abandoned, or reset --
/// leaves no slot and no span, only arena words no image owns. At the
/// terminator the image moves to the lowest gap that holds it, where
/// [`ImageStore::store`] would have put it, and is recorded there.
///
/// **A pixel takes the colour its register ends the body with**: the claim
/// holds colours while each names one register, and from the first data
/// that would break that -- a register painting in a colour clear or
/// another's, or a painted register redefined -- the register numbers,
/// which the terminator maps, so a redefinition recolours what the body
/// drew before it and a body that needs none pays nothing. A shared Sixel
/// (`?1070` reset) keeps the numbers too, a byte a pixel after its pixels
/// for its life, so a later shared Sixel's redefinition recolours it
/// ([`ImageStore::registers`]), once however many bodies of a read
/// redefined it ([`SixelStream::recolour`]); with no gap for both it is
/// refused at its size. A private one keeps nothing past its end.
#[derive(Clone)]
pub struct SixelStream {
    walk: Walk,
    /// The claim and what its words hold, colours or register numbers.
    st: ClaimState,
    /// The shared registers (`?1070` reset): what every shared Sixel so far
    /// defined, and which those are. Cleared by [`SixelStream::forget_shared`].
    shared: [Pixel; REGISTERS],
    shared_defined: RegisterSet,
    /// **The shared registers whose colour changed since the stored
    /// pictures last took theirs**, which [`SixelStream::recolour`] gives
    /// them in one pass.
    redefined: RegisterSet,
    registers: SixelRegisters,
    fill: SixelFill,
}

impl SixelStream {
    /// A stream with no Sixel begun.
    pub const NEW: SixelStream = SixelStream {
        walk: Walk::NEW,
        st: ClaimState::NEW,
        shared: [Pixel::CLEAR; REGISTERS],
        shared_defined: RegisterSet::NONE,
        redefined: RegisterSet::NONE,
        registers: SixelRegisters::Private,
        fill: SixelFill::Keep,
    };

    /// Begin a Sixel drawn at `aspect`, claiming `store`'s largest free gap.
    /// The registers start undefined and transparent, private to the image,
    /// and nothing limits how far down it goes.
    pub fn begin(&mut self, aspect: SixelAspect, store: &mut ImageStore) {
        self.begin_with(
            SixelBegin {
                aspect,
                ink: Pixel::CLEAR,
                registers: SixelRegisters::Private,
                floor: SixelEdge::Open,
                right: SixelEdge::Open,
                fill: SixelFill::Keep,
            },
            store,
        );
    }

    /// Begin a Sixel as `b` says, claiming `store`'s largest free gap. An
    /// undefined register paints `b.ink`; under
    /// [`SixelRegisters::Shared`] a register an earlier shared Sixel defined
    /// starts with that definition.
    pub fn begin_with(&mut self, b: SixelBegin, store: &mut ImageStore) {
        // A body that never reached its terminator lets its claim go here.
        self.abandon(store);
        let (at, len) = store.largest_gap();
        // Nothing written yet: a release beside it keeps no page.
        store.set_claim(Some((at, at)));
        self.walk = Walk::new(b.aspect);
        self.walk.regs = [b.ink; REGISTERS];
        if b.registers == SixelRegisters::Shared {
            for (r, c) in self.walk.regs.iter_mut().enumerate() {
                if self.shared_defined.has(r) {
                    *c = self.shared[r];
                }
            }
        }
        self.registers = b.registers;
        self.fill = b.fill;
        // Register 0 is selected and has not painted. A shared Sixel keeps
        // its numbers past its end, so it paints them from the start.
        self.st.inks = Inks::NEW;
        self.st.gap = Gap {
            at,
            len,
            floor: b.floor.px(),
            right: b.right.px(),
            ..Gap::NONE
        };
        if b.registers == SixelRegisters::Shared {
            self.st.inks.numbers();
        }
    }

    /// **Let the claim go**: the pages its written words touch that no
    /// image now holds go back ([`ImageStore::release_free`]), and the
    /// store stops keeping the gap's pages. What [`SixelStream::finish`]
    /// does as it ends, and what a terminal calls when a body ended
    /// without its terminator -- at its next control string or a reset,
    /// since `CAN`, `SUB` and an `ESC` that is not `ST` abandon a body
    /// with no action of their own. Nothing to do once let go. **A shared
    /// body's definitions stay in the shared set**, as each one took effect
    /// when it arrived, and as those of a read already ended did: where the
    /// reads fell does not change the set.
    #[cold]
    #[inline(never)]
    pub fn abandon(&mut self, store: &mut ImageStore) {
        if self.registers == SixelRegisters::Shared && self.st.gap.defined != RegisterSet::NONE {
            self.share();
        }
        let Gap { at, written, .. } = self.st.gap;
        store.set_claim(None);
        self.st.gap.written = 0;
        store.release_free(at, at + written);
    }

    /// Forget the shared registers: `RIS`. The pictures already stored
    /// take the colours owed them first ([`SixelStream::recolour`]), so a
    /// picture drawn after this never takes a colour defined before it.
    pub fn forget_shared(&mut self, store: &mut ImageStore) {
        self.recolour(store);
        self.shared_defined = RegisterSet::NONE;
    }

    /// One body byte the handler keeps ([`Sixel::body_byte`]).
    #[inline]
    pub fn put(&mut self, b: StreamByte, store: &mut ImageStore) {
        let mut sink = Claim {
            st: &mut self.st,
            store,
        };
        self.walk.put(b.raw_byte(), &mut sink);
    }

    /// The terminator. Refused and counted in the order the measured decode
    /// was -- too large, then empty, then a slot, then the arena -- or the
    /// rows packed to the image's width, moved to the lowest gap that holds
    /// them, and recorded: the image's id, width and height. Either way the
    /// claim is let go ([`SixelStream::abandon`]): what it wrote that the
    /// image does not now hold goes back, refused, moved or in place. The
    /// pictures already stored take the shared registers' new colours
    /// ([`SixelStream::recolour`]).
    pub fn finish(&mut self, store: &mut ImageStore) -> Option<(ImageId, SixelPx, SixelPx)> {
        let done = self.finish_deferring(store);
        self.recolour(store);
        done
    }

    /// **[`SixelStream::finish`] leaving the recolour for later**: the
    /// stored pictures take the shared registers this body redefined at
    /// the next [`SixelStream::recolour`], with every other body's since.
    /// What a terminal calls at a body's end, recolouring once a read, so a
    /// read of a hundred palettes costs one pass over the stored pictures
    /// and not a hundred. Nothing may read a stored shared picture's pixels
    /// in between -- a frame, a hard copy -- without the recolour first.
    pub fn finish_deferring(
        &mut self,
        store: &mut ImageStore,
    ) -> Option<(ImageId, SixelPx, SixelPx)> {
        let done = self.settle(store);
        self.abandon(store);
        done
    }

    /// [`SixelStream::finish`] up to letting the claim go.
    fn settle(&mut self, store: &mut ImageStore) -> Option<(ImageId, SixelPx, SixelPx)> {
        {
            let mut sink = Claim {
                st: &mut self.st,
                store,
            };
            self.walk.close(&mut sink);
        }
        // The shared set takes this Sixel's definitions whether or not it
        // is stored, a palette alone included, and owes the stored
        // pictures what it changes.
        if self.registers == SixelRegisters::Shared {
            self.share();
        }
        // Display mode keeps no row past the screen's bottom edge and no
        // column past its right edge: a band that crosses the bottom painted
        // the claim, and the rows past it are dropped here, where the
        // image's height is settled; the columns past the right were never
        // painted. So the image fits as the screen does, however far its
        // data reached.
        let (w, h) = match (
            SixelPx::at(self.walk.width.min(self.st.gap.right)),
            SixelPx::at(self.walk.height.min(self.st.gap.floor)),
        ) {
            (Some(w), Some(h)) => (w, h),
            _ => {
                store.refuse(ImageRefusal::TooLarge);
                return None;
            }
        };
        if w == SixelPx::ZERO || h == SixelPx::ZERO {
            store.refuse(ImageRefusal::Empty);
            return None;
        }
        // **The fill** grows the image to its region and sets every
        // position in it the data left clear to [`Pixel::FILL`]. The data's
        // extent is still what the image reports, for the cursor's advance:
        // 070-9 §11.3 moves the text by "the vertical motion commands
        // executed in the Sixel printing" (PDF p. 928), not by the fill.
        let region = match self.fill {
            SixelFill::Keep => None,
            SixelFill::Background { w: mw, h: mh, cell } => {
                // A side neither the raster nor the margins say -- the cell
                // size unknown -- is the data's reach, the image's own
                // rectangle.
                let (ph, pv) = self.st.gap.raster;
                let side = |raster: u32, margin: u32, data: u32| match (raster, margin) {
                    (0, 0) => data,
                    (0, m) => m,
                    (r, _) => r,
                };
                let fw = side(ph, mw, u32::from(w.raw()));
                let fh = side(pv, mh, u32::from(h.raw()));
                let cap = u32::from(u16::MAX);
                // **The fill goes on as the image scrolls**: data past the
                // rows left to the bottom margin scrolls the screen (070-9
                // §11.2.4), and the rows it scrolls in are filled from the
                // top of the band that overran them to the data's last row
                // rounded up to a whole cell, across the fill's width --
                // the rows left counted as filled "even if it wasn't all
                // filled to start with", so a raster extent shorter than
                // them leaves the rows between as they were, and "a multiple
                // of the cell height (this is to match the behavior of the
                // original hardware terminals)": Windows Terminal's
                // `_fillImageBackgroundWhenScrolled()`, whose Sixel was
                // checked against a VT340 (`[[source: wt-sixel-parser]]`).
                // The band's height is fixed once data begins. Nothing where
                // the rows left or the cell is unmeasured, and nothing in
                // display mode, whose floor keeps the data on the screen.
                let data_h = u32::from(h.raw());
                let scrolled = match (mh, cell) {
                    (0, _) | (_, 0) => None,
                    _ if data_h > mh => {
                        let to = data_h.div_ceil(cell).saturating_mul(cell);
                        let from = self.walk.aspect.band_top_at(mh);
                        Some((from, to.min(self.st.gap.floor).min(cap)))
                    }
                    _ => None,
                };
                Some((
                    fw.min(self.st.gap.right).min(cap),
                    fh.min(self.st.gap.floor).min(cap),
                    scrolled,
                ))
            }
        };
        let (fw, fh, scrolled) = region.unwrap_or((0, 0, None));
        let below = scrolled.map_or(0, |(_, to)| to);
        let (iw, ih) = (
            ImagePx::of(w.raw().max(fw as u16)),
            ImagePx::of(h.raw().max(fh as u16).max(below as u16)),
        );
        if region.is_some() && self.st.gap.held != Held::Lost {
            let mut sink = Claim {
                st: &mut self.st,
                store,
            };
            sink.grow(u32::from(iw.raw()), u32::from(ih.raw()));
        }
        if !store.has_free_slot() || self.st.gap.held == Held::Lost {
            // `store_at` counts which: the slot table first, as `store` does.
            let _ = store.store_at(None, iw, ih, ImageAlpha::OpaqueOrClear);
            return None;
        }
        let area = iw.index() * ih.index();
        {
            let mut sink = Claim {
                st: &mut self.st,
                store,
            };
            sink.restride(iw.index());
        }
        // **A shared Sixel keeps its register numbers for its life**, a
        // byte a pixel after its pixels ([`Plane`]), so a later Sixel that
        // redefines one recolours it; and with no gap for the pixels and
        // the numbers together it is refused at its size, as any picture
        // without room is, rather than stored to be drawn later in a
        // colour its registers no longer have.
        let plane = match self.registers {
            SixelRegisters::Shared => Plane::Registers,
            SixelRegisters::Private => Plane::None,
        };
        let span = area + plane.words(area);
        let at = self.st.gap.at;
        let Some(to) = store.lowest_gap(span) else {
            let _ = store.store_span(None, iw, ih, ImageAlpha::OpaqueOrClear, plane);
            return None;
        };
        if to != at {
            store.arena_mut().copy_within(at..at + area, to);
        }
        if self.st.inks.words == Words::Marks {
            self.map(&mut store.arena_mut()[to..to + span], area, plane);
        }
        if region.is_some() {
            let (iw, ih) = (iw.index(), ih.index());
            let fw = (fw as usize).min(iw);
            let px = &mut store.arena_mut()[to..to + area];
            for (a, b) in [(0, fh), scrolled.unwrap_or((0, 0))] {
                for y in a as usize..(b as usize).min(ih) {
                    for p in &mut px[y * iw..y * iw + fw] {
                        if *p == Pixel::CLEAR {
                            *p = Pixel::FILL;
                        }
                    }
                }
            }
        }
        let id = store.store_span(Some(to), iw, ih, ImageAlpha::OpaqueOrClear, plane)?;
        if region.is_some() {
            let ground = ImageGround::Filled {
                data_w: ImagePx::of(w.raw()),
                data_h: ImagePx::of(h.raw()),
            };
            store.set_ground(id, ground);
            store.sixels_filled += 1;
        }
        Some((id, w, h))
    }

    /// **Every register number in `span`'s first `area` words becomes the
    /// colour its register ends the body with**, where the body went on in
    /// numbers ([`Inks`]), and for a shared Sixel
    /// is kept in the plane after them, a byte a pixel. A redefinition
    /// inside the body so recolours the pixels drawn before it, as a
    /// VT340's colour map does -- "The pixels on the screen are kept in
    /// memory as color indices which map to the actual color to display"
    /// (`[[source: vt340test-colormap]]`) -- and as xterm, which maps a
    /// pixel's register when it draws and defines a register as the
    /// command arrives (`update_color_register()`,
    /// `[[source: xterm-graphics-sixel]]`). Nothing draws the claim before
    /// this. A clear word stays clear; under a fill it becomes the fill
    /// after this, as a pixel drawn in a clear ink does in a private
    /// Sixel. **In a shared one a pixel drawn in a clear ink** -- an
    /// undefined register while the terminal has not said its foreground
    /// -- is [`Pixel::UNSAID`], drawn as the clear word is, so a later
    /// definition of its register still finds it, as it finds one drawn in
    /// a foreground said: a clear word in the plane's picture could be a
    /// position the data left at zero, which no register drew.
    fn map(&self, span: &mut [Pixel], area: usize, plane: Plane) {
        let (px, rest) = span.split_at_mut(area);
        match plane {
            Plane::None => {
                let regs = &self.walk.regs;
                for p in px.iter_mut().filter(|p| **p != Pixel::CLEAR) {
                    *p = regs[usize::from(Register::of(*p))];
                }
            }
            Plane::Registers => {
                let regs = self.walk.regs.map(|c| match c {
                    Pixel::CLEAR => Pixel::UNSAID,
                    c => c,
                });
                let numbers = Pixel::bytes_mut(rest);
                for (p, n) in px.iter_mut().zip(numbers.iter_mut()) {
                    *n = 0;
                    if *p != Pixel::CLEAR {
                        *n = Register::of(*p);
                        *p = regs[usize::from(*n)];
                    }
                }
            }
        }
    }

    /// **The shared set takes the definitions this Sixel made since it
    /// last took them** (`?1070` reset), and every register whose colour
    /// that changed -- one never defined before included, drawn in the
    /// ink -- is owed to the pictures drawn with it, which
    /// [`SixelStream::recolour`] pays. At each read's end while the body is
    /// open ([`SixelStream::recolour`]), at its end, `ST` or a cut, and
    /// where it is let go with neither ([`SixelStream::abandon`]), so the
    /// set ends the same wherever the reads fell. A definition is applied
    /// when the byte after it arrives, which ends its parameters.
    #[cold]
    #[inline(never)]
    fn share(&mut self) {
        let mut changed = RegisterSet::NONE;
        for r in 0..REGISTERS {
            if !self.st.gap.defined.has(r) {
                continue;
            }
            let c = self.walk.regs[r];
            if !self.shared_defined.has(r) || self.shared[r] != c {
                changed.add(r);
            }
            self.shared[r] = c;
        }
        self.shared_defined = self.shared_defined.with(self.st.gap.defined);
        self.redefined = self.redefined.with(changed);
        self.st.gap.defined = RegisterSet::NONE;
    }

    /// **The stored pictures take the shared registers' new colours**:
    /// every picture kept with its register numbers that was drawn with a
    /// register whose colour a shared Sixel changed since the last call is
    /// recoloured to the colour the register has now
    /// ([`ImageStore::recolour`]), one pass however many bodies changed
    /// it: "You can draw an image, then change its appearance by changing
    /// the associated output map location ... you do not have to redraw an
    /// image to change its color" (`[[source: vt3xx-screen-control]]`,
    /// "Output Mapping"), and xterm's shared graphics, which draw from the
    /// one shared set (`[[source: xterm-graphics-c]]`). Taking the last
    /// colour once leaves what taking each in turn would: a picture
    /// stored between two definitions drew with the colour the set held
    /// then, which the later one replaces either way. A terminal calls it
    /// at each read's end and before anything in a read looks at a
    /// picture's pixels.
    ///
    /// **A shared body still open gives what it defined so far first**,
    /// into the shared set, so the frame after a read shows every
    /// definition the read carried, as a VT340's colour map, read at each
    /// refresh, does: "During display refresh, the active color map is used
    /// to translate color values from display memory to colors that appear
    /// on the screen" (`[[source: dec-std-070]]` 070-5 §5.6.3.3). A frame is
    /// drawn between reads and never inside one, so the last of two
    /// definitions in one read is what any refresh between them would have
    /// left on every frame. One pass a read however many definitions it
    /// carried; nothing scanned when no colour changed.
    #[inline]
    pub fn recolour(&mut self, store: &mut ImageStore) {
        if self.registers == SixelRegisters::Shared && self.st.gap.defined != RegisterSet::NONE {
            self.share();
        }
        if self.redefined != RegisterSet::NONE {
            self.recolour_owed(store);
        }
    }

    /// [`SixelStream::recolour`] with a colour owed.
    #[cold]
    #[inline(never)]
    fn recolour_owed(&mut self, store: &mut ImageStore) {
        let (changed, shared) = (self.redefined, &self.shared);
        store.recolour(|r| {
            let r = usize::from(r);
            changed.has(r).then(|| shared[r])
        });
        self.redefined = RegisterSet::NONE;
    }

    /// One past the highest word of its claim this Sixel has written so far
    /// -- cleared, painted or moved -- which is how much of the arena it
    /// commits. Zero before any data and once the claim is let go.
    pub fn written(&self) -> usize {
        self.st.gap.written
    }
}

/// A colour register from RGB percentages (`Pu = 2`): each of `Px`, `Py`, `Pz`
/// is 0 to 100, scaled to a 0..=255 channel with rounding.
pub(crate) fn rgb(r: u16, g: u16, b: u16) -> Pixel {
    Pixel::opaque(pct(r), pct(g), pct(b))
}

/// A colour register from DEC HLS (`Pu = 1`): hue `h` in degrees, lightness
/// `l` and saturation `s` in percent; `None` when any is past its range
/// (`[[source: vt3xx-sixel]]` Table 14-1: 0-360, 0-100, 0-100).
///
/// **DEC's hue origin is blue.** `[[source: vt3xx-screen-control]]`, the
/// chapter the Sixel chapter defers its colours to: "Blue: 0 degrees Red: 120
/// degrees Green: 240 degrees". DEC gives no formula, calling HLS
/// "universally recognized" (`[[source: vt3xx-sixel]]`), so this is the
/// double-hexcone conversion `[[source: css-color-4-hsl]]` prints, whose red
/// is at 0, with the hue turned 120 degrees back. It reproduces that
/// chapter's Table 2-3, the VT340's sixteen default colours given as HLS and
/// RGB both, within one percent on every row.
///
/// In integers, since `no_std` has no `f32::round`: with `K = (30 n + h) mod
/// 360` and `m = clamp(min(K - 90, 270 - K), -30, 30)`, CSS's
/// `l - a * max(-1, min(k - 3, 9 - k, 1))` is `(3000 l - s * min(l, 100 - l)
/// * m) / 3000` percent, which stays within 0..=100, so the channel is that
/// times 255/100, rounded half up as [`pct`] rounds.
pub(crate) fn hls(h: u16, l: u16, s: u16) -> Option<Pixel> {
    if h > 360 || l > 100 || s > 100 {
        return None;
    }
    let h = (i32::from(h) + 240) % 360;
    let (l, s) = (i32::from(l), i32::from(s));
    let a = s * l.min(100 - l);
    let chan = |n: i32| {
        let k = (30 * n + h) % 360;
        let m = (k - 90).min(270 - k).clamp(-30, 30);
        let v = 3000 * l - a * m;
        let c = (255 * v + 150_000) / 300_000;
        Chan::from_u8(u8::try_from(c).unwrap_or(u8::MAX))
    };
    Some(Pixel::opaque(chan(0), chan(8), chan(4)))
}

/// A 0-100 percentage as a channel, clamped and rounded.
fn pct(v: u16) -> Chan {
    let v = v.min(100);
    // 0..=100 -> 0..=255, `(v * 255 + 50) / 100`, all within `u16`.
    let scaled = (v * 255 + 50) / 100;
    Chan::from_u8(u8::try_from(scaled).unwrap_or(u8::MAX))
}
