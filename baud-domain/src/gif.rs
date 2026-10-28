//! **GIF, decoded to frames** (`[[source: gif89a]]`): GIF87a and GIF89a,
//! the variable-length-code LZW of Appendix F, global and local colour
//! tables, interlaced images (Appendix E), the transparency index, the four
//! disposal methods, and the frames an animation shows with their delays,
//! the User Input Flag and the NETSCAPE2.0 loop count
//! (`[[source: netscape-2-0-extension]]`).
//!
//! **Why it is here.** An iTerm2 inline picture is "Any image format that
//! macOS supports" (`[[source: iterm2-images]]`), and iTerm2 plays an
//! animated GIF's frames (`[[source: iterm2-964a75d-image-with-data]]`);
//! with no engine dependency the decoder is baud's own, as PNG's and
//! JPEG's are.
//!
//! **The logical screen is a canvas, and a frame is what it shows.** Each
//! graphic -- an image, or a Plain Text Extension -- is rendered onto it in
//! the stream's order, "without delays other than those specified in the
//! control information" (8), and the canvas becomes a frame when its
//! graphic's Delay Time is not zero, or it waits for input: a delay of 0 is
//! no wait, so a graphic with none is composited into the next and never
//! shown alone, and a stream that ends on one ends showing it. A stream whose
//! every delay is 0 is one still picture, every graphic composited, as the
//! text says; kitty's `icat` and browsers play each such graphic for ten
//! hundredths (`[[source: kitty-imaging-5a098df-gifmeta]]`), which would be a
//! departure. A stream of one graphic is a still picture whatever its delay.
//! Where no image covers the screen, and where disposal 2 restores, it shows
//! "the Background Color" of the global table, opaque, clear only with no
//! global table (18 vii, 23 iv).
//!
//! **What is not drawn.** A Plain Text Extension's text (25) is not
//! rendered -- its Graphic Control Extension is still taken, so its delay
//! and disposal apply -- and is counted by the caller.
//!
//! **Malformed input never panics**, and the work is bounded: every read
//! is checked against the file, an LZW code past the table ends its
//! image, an image's pixels past its rectangle are dropped and those its
//! data never reaches keep the canvas, a stream cut short shows what
//! arrived, and the frames are counted from the file before any is made,
//! so the caller reserves them once.

use crate::png::Size;

/// Why a GIF could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GifError {
    /// Not a GIF, or one with no graphic to show.
    Malformed,
    /// A caller buffer smaller than [`Gif::scratch_len`].
    Buffer,
}

/// **Whether a frame waits for the user**, GIF89a's User Input Flag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GifWait {
    Delay,
    Input,
}

/// One frame of the canvas: its RGBA bytes, the logical screen's size, its
/// gap in milliseconds and whether it waits for input.
pub struct GifFrame<'a> {
    pub rgba: &'a [u8],
    pub gap_ms: u32,
    pub wait: GifWait,
}

/// A colour table: where its entries start and how many it has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Table {
    at: usize,
    n: usize,
}

/// "Disposal Method - Indicates the way in which the graphic is to be
/// treated after being displayed" (23 iv); 4 to 7, "To be defined", as 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Disposal {
    None,
    Background,
    Previous,
}

/// A Graphic Control Extension's fields (23).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Control {
    disposal: Disposal,
    wait: GifWait,
    /// Hundredths of a second.
    delay: u16,
    transparent: Option<u8>,
}

impl Control {
    const NONE: Control = Control {
        disposal: Disposal::None,
        wait: GifWait::Delay,
        delay: 0,
        transparent: None,
    };
}

/// A graphic's rectangle on the logical screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

/// Whether an image's rows come in Appendix E's four passes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rows {
    InOrder,
    Interlaced,
}

/// A block of the data stream, as [`next_block`] reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Block {
    Control(Control),
    /// An image: its rectangle, its table, its rows, and where its LZW
    /// minimum code size is.
    Image(Rect, Option<Table>, Rows, usize),
    /// A Plain Text Extension, its text grid's rectangle.
    Text(Rect),
    /// An application extension's NETSCAPE2.0 loop count, or ANIMEXTS1.0's,
    /// which Firefox reads as the same block
    /// (`[[source: firefox-774fa0c-gif-decoder]]`).
    Loops(u16),
    /// Anything else: a comment, an unknown extension.
    Other,
}

fn le16(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(i)?, *d.get(i + 1)?]))
}

/// **Whether an image's LZW minimum code size at `at` is one GIF allows**:
/// 2 to 8, "the minimum number of bits required to represent the set of
/// actual pixel values", which no table of 256 entries passes, and 2 for
/// "black & white images which have one color bit". A file with another
/// is refused whole, as macOS's decoder refuses it.
fn code_size_ok(d: &[u8], at: usize) -> bool {
    d.get(at).is_some_and(|m| (2..=8).contains(m))
}

/// The position after the data sub-blocks at `pos` and their terminator,
/// or `None` where the file ends first.
fn skip_blocks(d: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let n = usize::from(*d.get(pos)?);
        if n == 0 {
            return Some(pos + 1);
        }
        pos += 1 + n;
    }
}

/// A colour table of `flags`'s size at `pos`, "3 x 2^(Size of Color
/// Table+1)" bytes (18 vi), if its flag is set.
fn table(d: &[u8], flags: u8, pos: usize) -> Option<Option<Table>> {
    if flags & 0x80 == 0 {
        return Some(None);
    }
    let n = 2usize << (flags & 7);
    d.get(pos..pos + 3 * n)?;
    Some(Some(Table { at: pos, n }))
}

/// **The block at `pos`** and the position after it; `None` at the
/// trailer, the file's end or a byte that starts no block. An image cut
/// short is still returned, its end the file's.
fn next_block(d: &[u8], pos: usize) -> Option<(Block, usize)> {
    match *d.get(pos)? {
        0x21 => {
            let label = *d.get(pos + 1)?;
            let body = pos + 2;
            let end = skip_blocks(d, body)?;
            let first = d.get(body + 1..body + 1 + usize::from(*d.get(body)?));
            let block = match (label, first) {
                (0xF9, Some(g)) if g.len() >= 4 => {
                    let disposal = match (g[0] >> 2) & 7 {
                        2 => Disposal::Background,
                        3 => Disposal::Previous,
                        _ => Disposal::None,
                    };
                    let wait = match g[0] & 2 {
                        0 => GifWait::Delay,
                        _ => GifWait::Input,
                    };
                    let transparent = (g[0] & 1 == 1).then_some(g[3]);
                    Block::Control(Control {
                        disposal,
                        wait,
                        delay: u16::from_le_bytes([g[1], g[2]]),
                        transparent,
                    })
                }
                (0x01, Some(t)) if t.len() >= 8 => Block::Text(Rect {
                    x: usize::from(u16::from_le_bytes([t[0], t[1]])),
                    y: usize::from(u16::from_le_bytes([t[2], t[3]])),
                    w: usize::from(u16::from_le_bytes([t[4], t[5]])),
                    h: usize::from(u16::from_le_bytes([t[6], t[7]])),
                }),
                (0xFF, Some(id)) if id == b"NETSCAPE2.0" || id == b"ANIMEXTS1.0" => {
                    let sub = body + 1 + 11;
                    match d.get(sub..sub + 4) {
                        Some([3, 1, lo, hi]) => Block::Loops(u16::from_le_bytes([*lo, *hi])),
                        _ => Block::Other,
                    }
                }
                _ => Block::Other,
            };
            Some((block, end))
        }
        0x2C => {
            let rect = Rect {
                x: usize::from(le16(d, pos + 1)?),
                y: usize::from(le16(d, pos + 3)?),
                w: usize::from(le16(d, pos + 5)?),
                h: usize::from(le16(d, pos + 7)?),
            };
            let flags = *d.get(pos + 9)?;
            let lct = table(d, flags, pos + 10)?;
            let data = pos + 10 + lct.map_or(0, |t| 3 * t.n);
            let rows = match flags & 0x40 {
                0 => Rows::InOrder,
                _ => Rows::Interlaced,
            };
            d.get(data)?;
            let end = skip_blocks(d, data + 1).unwrap_or(d.len());
            Some((Block::Image(rect, lct, rows, data), end))
        }
        _ => None,
    }
}

/// **A parsed GIF**: its logical screen, global table and background, the
/// first block, and what the walk of its blocks found: how many frames it
/// shows and its loop count.
#[derive(Clone, Copy, Debug)]
pub struct Gif<'a> {
    data: &'a [u8],
    width: u16,
    height: u16,
    gct: Option<Table>,
    bg: u8,
    first: usize,
    frames: u32,
    graphics: u32,
    loops: Option<u16>,
    texts: u32,
}

impl<'a> Gif<'a> {
    /// Whether `d` begins as a GIF does: "GIF87a" or "GIF89a" (17).
    pub fn sniff(d: &[u8]) -> bool {
        matches!(d.get(..6), Some(b"GIF87a" | b"GIF89a"))
    }

    /// **The header and the walk of the blocks**: the logical screen
    /// descriptor and global table, then every block to the trailer or the
    /// file's end, counting the frames the decode will make. A stream with
    /// no graphic is refused.
    pub fn parse(data: &'a [u8]) -> Result<Gif<'a>, GifError> {
        if !Gif::sniff(data) {
            return Err(GifError::Malformed);
        }
        let width = le16(data, 6).ok_or(GifError::Malformed)?;
        let height = le16(data, 8).ok_or(GifError::Malformed)?;
        let flags = *data.get(10).ok_or(GifError::Malformed)?;
        let bg = *data.get(11).ok_or(GifError::Malformed)?;
        data.get(12).ok_or(GifError::Malformed)?;
        let gct = table(data, flags, 13).ok_or(GifError::Malformed)?;
        let first = 13 + gct.map_or(0, |t| 3 * t.n);
        if width == 0 || height == 0 {
            return Err(GifError::Malformed);
        }
        let mut g = Gif {
            data,
            width,
            height,
            gct,
            bg,
            first,
            frames: 0,
            graphics: 0,
            loops: None,
            texts: 0,
        };
        // Each graphic's delay and wait, in order: the frames come from
        // them.
        let mut graphics = 0u32;
        let mut shown = 0u32;
        let mut last_shown = false;
        let mut control = Control::NONE;
        let mut pos = first;
        while let Some((block, next)) = next_block(data, pos) {
            pos = next;
            match block {
                Block::Control(c) => control = c,
                Block::Loops(n) => g.loops = g.loops.or(Some(n)),
                Block::Image(.., at) if !code_size_ok(data, at) => {
                    return Err(GifError::Malformed);
                }
                Block::Image(..) | Block::Text(_) => {
                    if matches!(block, Block::Text(_)) {
                        g.texts += 1;
                    }
                    graphics += 1;
                    last_shown = control.delay != 0 || control.wait == GifWait::Input;
                    if last_shown {
                        shown += 1;
                    }
                    control = Control::NONE;
                }
                Block::Other => {}
            }
        }
        if graphics == 0 {
            return Err(GifError::Malformed);
        }
        g.graphics = graphics;
        // The graphics that wait, and the one the stream ends on if it
        // does not: one graphic is one frame, a still picture, whatever
        // its delay, and so is a stream with no wait in it.
        g.frames = shown + u32::from(!last_shown);
        Ok(g)
    }

    /// The logical screen's size.
    pub fn size(&self) -> Size {
        Size {
            width: u32::from(self.width),
            height: u32::from(self.height),
        }
    }

    /// How many frames [`Gif::decode`] makes.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// **How many times the animation plays**, zero for ever: "an
    /// iteration count of zero indicates infinite", and a count of `n`
    /// loops `n` times after the first play, as Firefox and kitty's `icat`
    /// read it; with no NETSCAPE2.0 block the stream is processed once.
    pub fn plays(&self) -> u32 {
        match self.loops {
            None => 1,
            Some(0) => 0,
            Some(n) => u32::from(n) + 1,
        }
    }

    /// How many Plain Text Extensions it carries, which are not drawn.
    pub fn texts_not_drawn(&self) -> u32 {
        self.texts
    }

    /// The bytes of work [`Gif::decode`] needs: the canvas and the copy
    /// disposal 3 restores from, four bytes a pixel each.
    pub fn scratch_len(&self) -> usize {
        usize::from(self.width) * usize::from(self.height) * 8
    }

    /// The colour of entry `i` of `t`, opaque; black past its end, or with
    /// no table, which GIF89a leaves to the decoder ("the decoder is free
    /// to use a system color table").
    fn colour(&self, t: Option<Table>, i: u8) -> [u8; 4] {
        let i = usize::from(i);
        match t.filter(|t| i < t.n) {
            Some(t) => {
                let at = t.at + 3 * i;
                [self.data[at], self.data[at + 1], self.data[at + 2], 255]
            }
            None => [0, 0, 0, 255],
        }
    }

    /// The Background Color, opaque, or clear with no global table.
    fn background(&self) -> [u8; 4] {
        match self.gct {
            Some(_) => self.colour(self.gct, self.bg),
            None => [0; 4],
        }
    }

    /// **Every frame, in order, to `emit`**, with `scratch` of at least
    /// [`Gif::scratch_len`] bytes for the canvas: each graphic rendered and
    /// the canvas given out where [`Gif::parse`] counted a frame, then the
    /// graphic's disposal applied, "after being displayed". The last
    /// graphic, if not shown alone, is given out as the stream ends on it,
    /// its gap zero. `emit` stops the decode by returning false.
    pub fn decode(
        &self,
        scratch: &mut [u8],
        mut emit: impl FnMut(GifFrame<'_>) -> bool,
    ) -> Result<(), GifError> {
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        let work = scratch
            .get_mut(..self.scratch_len())
            .ok_or(GifError::Buffer)?;
        let (canvas, saved) = work.split_at_mut(w * h * 4);
        let bg = self.background();
        for px in canvas.chunks_exact_mut(4) {
            px.copy_from_slice(&bg);
        }
        let mut lzw = Lzw::NEW;
        let mut control = Control::NONE;
        let mut graphic = 0u32;
        let mut pos = self.first;
        while let Some((block, next)) = next_block(self.data, pos) {
            pos = next;
            let rect = match block {
                Block::Control(c) => {
                    control = c;
                    continue;
                }
                Block::Image(r, ..) | Block::Text(r) => r,
                _ => continue,
            };
            let c = core::mem::replace(&mut control, Control::NONE);
            // The part of the graphic on the screen.
            let (x0, y0) = (rect.x.min(w), rect.y.min(h));
            let (x1, y1) = ((rect.x + rect.w).min(w), (rect.y + rect.h).min(h));
            if c.disposal == Disposal::Previous {
                for y in y0..y1 {
                    let r = (y * w + x0) * 4..(y * w + x1) * 4;
                    saved[r.clone()].copy_from_slice(&canvas[r]);
                }
            }
            if let Block::Image(r, lct, rows, data) = block {
                let table = lct.or(self.gct);
                let mut put = Put {
                    gif: self,
                    canvas,
                    w,
                    h,
                    rect: r,
                    rows,
                    table,
                    transparent: c.transparent,
                    at: 0,
                };
                lzw.decode(self.data, data, &mut put);
            }
            graphic += 1;
            let shows = c.delay != 0 || c.wait == GifWait::Input;
            if shows || graphic == self.graphics {
                let frame = GifFrame {
                    rgba: canvas,
                    gap_ms: u32::from(c.delay) * 10,
                    wait: c.wait,
                };
                if !emit(frame) {
                    return Ok(());
                }
            }
            match c.disposal {
                Disposal::None => {}
                Disposal::Background => {
                    for y in y0..y1 {
                        for px in canvas[(y * w + x0) * 4..(y * w + x1) * 4].chunks_exact_mut(4) {
                            px.copy_from_slice(&bg);
                        }
                    }
                }
                Disposal::Previous => {
                    for y in y0..y1 {
                        let r = (y * w + x0) * 4..(y * w + x1) * 4;
                        canvas[r.clone()].copy_from_slice(&saved[r]);
                    }
                }
            }
        }
        Ok(())
    }
}

/// **Where an image's decoded indices go**: each the next pixel of its
/// rectangle, in rows top to bottom or Appendix E's passes, its colour put
/// on the canvas where it is on the screen, unless it is the transparency
/// index, which "the corresponding pixel of the display device is not
/// modified" (23 viii).
struct Put<'g, 'c> {
    gif: &'g Gif<'g>,
    canvas: &'c mut [u8],
    w: usize,
    h: usize,
    rect: Rect,
    rows: Rows,
    table: Option<Table>,
    transparent: Option<u8>,
    at: usize,
}

impl Put<'_, '_> {
    /// Whether every pixel of the rectangle has its index.
    fn full(&self) -> bool {
        self.at >= self.rect.w * self.rect.h
    }

    fn pixel(&mut self, i: u8) {
        let (rw, rh) = (self.rect.w, self.rect.h);
        if rw == 0 || self.full() {
            return;
        }
        let (n, col) = (self.at / rw, self.at % rw);
        self.at += 1;
        let row = match self.rows {
            Rows::InOrder => n,
            Rows::Interlaced => {
                // "Every 8th. row, starting with row 0", then from row 4,
                // every 4th from row 2, every 2nd from row 1.
                let p1 = rh.div_ceil(8);
                let p2 = (rh + 3) / 8;
                let p3 = (rh + 1) / 4;
                match n {
                    n if n < p1 => n * 8,
                    n if n < p1 + p2 => (n - p1) * 8 + 4,
                    n if n < p1 + p2 + p3 => (n - p1 - p2) * 4 + 2,
                    n => (n - p1 - p2 - p3) * 2 + 1,
                }
            }
        };
        if Some(i) == self.transparent {
            return;
        }
        let (x, y) = (self.rect.x + col, self.rect.y + row);
        if x >= self.w || y >= self.h {
            return;
        }
        let at = (y * self.w + x) * 4;
        self.canvas[at..at + 4].copy_from_slice(&self.gif.colour(self.table, i));
    }
}

/// The bits of an image's data sub-blocks, least significant first, "as if
/// they were packed right to left" (Appendix F).
struct Codes<'a> {
    d: &'a [u8],
    /// The next byte, and the bytes left in its sub-block.
    pos: usize,
    left: usize,
    acc: u32,
    n: u32,
}

impl Codes<'_> {
    /// The next `width` bits, or `None` once the sub-blocks end.
    fn next(&mut self, width: u32) -> Option<u16> {
        while self.n < width {
            if self.left == 0 {
                let len = usize::from(*self.d.get(self.pos)?);
                if len == 0 {
                    return None;
                }
                self.pos += 1;
                self.left = len;
            }
            let b = *self.d.get(self.pos)?;
            self.pos += 1;
            self.left -= 1;
            self.acc |= u32::from(b) << self.n;
            self.n += 8;
        }
        let v = (self.acc & ((1 << width) - 1)) as u16;
        self.acc >>= width;
        self.n -= width;
        Some(v)
    }
}

/// The most codes a table holds: twelve bits.
const LZW_MAX: usize = 4096;

/// **The LZW string table** (Appendix F): each code's prefix code and last
/// index, and its first index, so a string is written from its last index
/// back.
struct Lzw {
    prefix: [u16; LZW_MAX],
    suffix: [u8; LZW_MAX],
    first: [u8; LZW_MAX],
    stack: [u8; LZW_MAX],
}

impl Lzw {
    const NEW: Lzw = Lzw {
        prefix: [0; LZW_MAX],
        suffix: [0; LZW_MAX],
        first: [0; LZW_MAX],
        stack: [0; LZW_MAX],
    };

    /// **An image's data at `pos`**, its minimum code size and its
    /// sub-blocks, into `put`: the Clear code "resets all ... tables to a
    /// start-up state", the End of Information code ends it, codes grow a
    /// bit "Whenever the LZW code value would exceed the current code
    /// length", up to twelve, and a full table takes no more entries until
    /// a Clear. A code past the table ends the image where it is; a minimum
    /// code size outside 2 to 8 has refused the file ([`code_size_ok`]).
    fn decode(&mut self, d: &[u8], pos: usize, put: &mut Put<'_, '_>) {
        let Some(&m) = d.get(pos) else {
            return;
        };
        if !(2..=8).contains(&m) {
            return;
        }
        let clear = 1u16 << m;
        let eoi = clear + 1;
        for c in 0..clear {
            self.suffix[usize::from(c)] = c as u8;
            self.first[usize::from(c)] = c as u8;
        }
        let mut codes = Codes {
            d,
            pos: pos + 1,
            left: 0,
            acc: 0,
            n: 0,
        };
        let mut width = u32::from(m) + 1;
        let mut next = clear + 2;
        let mut prev: Option<u16> = None;
        while !put.full() {
            let Some(code) = codes.next(width) else {
                return;
            };
            if code == clear {
                width = u32::from(m) + 1;
                next = clear + 2;
                prev = None;
                continue;
            }
            if code == eoi {
                return;
            }
            let Some(p) = prev else {
                if code >= clear {
                    return;
                }
                put.pixel(code as u8);
                prev = Some(code);
                continue;
            };
            let first = match code {
                c if c < next => self.first[usize::from(c)],
                c if c == next && usize::from(next) < LZW_MAX => self.first[usize::from(p)],
                _ => return,
            };
            if usize::from(next) < LZW_MAX {
                let n = usize::from(next);
                self.prefix[n] = p;
                self.suffix[n] = first;
                self.first[n] = self.first[usize::from(p)];
                next += 1;
                if usize::from(next) == 1 << width && width < 12 {
                    width += 1;
                }
            }
            self.write(code, clear, put);
            prev = Some(code);
        }
    }

    /// The string of `code`, its indices from first to last, into `put`.
    fn write(&mut self, code: u16, clear: u16, put: &mut Put<'_, '_>) {
        let mut n = 0;
        let mut c = code;
        while n < LZW_MAX {
            self.stack[n] = self.suffix[usize::from(c)];
            n += 1;
            if c < clear {
                break;
            }
            c = self.prefix[usize::from(c)];
        }
        for k in (0..n).rev() {
            put.pixel(self.stack[k]);
        }
    }
}
