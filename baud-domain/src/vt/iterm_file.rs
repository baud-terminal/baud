//! **iTerm2's inline images**, `OSC 1337 ; File=args:base64` and its
//! multipart form, `MultipartFile=args`, `FilePart=base64`, `FileEnd`
//! (`[[source: iterm2-images]]`): the arguments read as iTerm2's source reads
//! them, the base64 decoded into the caller's scratch as it arrives, and the
//! cells and device pixels a picture takes, by iTerm2's own arithmetic
//! (`[[source: iterm2-964a75d-inline-image-helper]]`).
//!
//! **What is here is the protocol; placing the picture is the core's.** The
//! stream keeps the file's octets in the scratch the kitty stream lends for
//! its PNGs, which the core keeps from being in both hands at once, and
//! [`file_geometry`] is a pure function of the arguments, the picture and
//! the screen, so every drawing path draws the one result.

use crate::jpeg::Jpeg;
use crate::png::Png;
use crate::{Base64, Base64Step, Col, Cols, ImageAlpha, ImageId, ImagePx, ImageStore, Pixel, Rows};

/// iTerm2's `boolValue`, which its source reads `inline` and
/// `preserveAspectRatio` with: true for a value whose first character past
/// spaces is `Y`, `y`, `T`, `t` or a digit 1 to 9, as `NSString`'s.
fn bool_value(v: &[u8]) -> bool {
    let mut i = 0;
    while i < v.len() && v[i] == b' ' {
        i += 1;
    }
    match v.get(i) {
        Some(b'Y' | b'y' | b'T' | b't' | b'1'..=b'9') => true,
        Some(b'+' | b'-') => {
            // A sign, then zeros, then a non-zero digit is true.
            let mut j = i + 1;
            while j < v.len() && v[j] == b'0' {
                j += 1;
            }
            matches!(v.get(j), Some(b'1'..=b'9'))
        }
        Some(b'0') => {
            let mut j = i;
            while j < v.len() && v[j] == b'0' {
                j += 1;
            }
            matches!(v.get(j), Some(b'1'..=b'9'))
        }
        _ => false,
    }
}

/// `NSString`'s `intValue` over the digits iTerm2 reads a size with: leading
/// spaces, a sign, the digits up to the first that is not one, 0 for none.
fn int_value(v: &[u8]) -> i64 {
    let mut i = 0;
    while i < v.len() && v[i] == b' ' {
        i += 1;
    }
    let neg = match v.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut n: i64 = 0;
    while i < v.len() && v[i].is_ascii_digit() {
        n = (n * 10 + i64::from(v[i] - b'0')).min(i64::from(i32::MAX) + 1);
        i += 1;
    }
    match neg {
        true => -n,
        false => n,
    }
}

/// A width or a height, as `width=` and `height=` give it: "N: N character
/// cells. Npx: N pixels. N%: N percent of the session's width or height.
/// auto: The image's inherent size".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileExtent {
    Auto,
    Cells(i64),
    Pixels(i64),
    Percent(i64),
}

impl FileExtent {
    /// iTerm2's reading: exactly `auto`, else the unit by its suffix, the
    /// number by `intValue` -- so `50%` is 50 percent and `3em` 3 cells.
    fn of(v: &[u8]) -> FileExtent {
        let n = int_value(v);
        if v == b"auto" {
            FileExtent::Auto
        } else if v.ends_with(b"px") {
            FileExtent::Pixels(n)
        } else if v.ends_with(b"%") {
            FileExtent::Percent(n)
        } else {
            FileExtent::Cells(n)
        }
    }
}

/// Whether a file is shown or downloaded: `inline`, "Defaults to 0".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileShown {
    /// `inline=1`: "displayed inline".
    Inline,
    /// "Otherwise, it will be downloaded with no visual representation".
    Download,
}

/// `preserveAspectRatio`: "If set to 0, then the image's inherent aspect ratio
/// will not be respected; otherwise, it will fill the specified width and
/// height as much as possible without stretching. Defaults to 1."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileAspect {
    Kept,
    Stretched,
}

/// **A file's arguments**, the `;`-separated `key=value` pairs before the
/// `:`, iTerm2's defaults where a key is absent: `width` and `height` auto,
/// the aspect kept, not inline. A later key replaces an earlier one, as
/// iTerm2's dictionary does. `name`, `size`, `type`, `mode` and the insets
/// are read by iTerm2 for its download panel, its text documents and an
/// inset its arithmetic does not use; baud has none of the three.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileArgs {
    pub width: FileExtent,
    pub height: FileExtent,
    pub aspect: FileAspect,
    pub shown: FileShown,
}

impl FileArgs {
    pub const DEFAULT: FileArgs = FileArgs {
        width: FileExtent::Auto,
        height: FileExtent::Auto,
        aspect: FileAspect::Kept,
        shown: FileShown::Download,
    };

    /// The arguments in `header`, the bytes between `File=` (or
    /// `MultipartFile=`) and the `:`.
    pub fn parse(header: &[u8]) -> FileArgs {
        let mut a = FileArgs::DEFAULT;
        for part in header.split(|&b| b == b';') {
            let Some(eq) = part.iter().position(|&b| b == b'=').filter(|&i| i > 0) else {
                continue;
            };
            let (k, v) = (&part[..eq], &part[eq + 1..]);
            match k {
                b"width" => a.width = FileExtent::of(v),
                b"height" => a.height = FileExtent::of(v),
                b"preserveAspectRatio" => {
                    a.aspect = match bool_value(v) {
                        true => FileAspect::Kept,
                        false => FileAspect::Stretched,
                    }
                }
                b"inline" => {
                    a.shown = match bool_value(v) {
                        true => FileShown::Inline,
                        false => FileShown::Download,
                    }
                }
                _ => {}
            }
        }
        a
    }
}

/// Whether a file came in one message or in parts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileParts {
    /// `File=args:base64`, ended by its terminator.
    Whole,
    /// `MultipartFile=args`, its `FilePart`s, ended by `FileEnd`.
    Multipart,
}

/// Where the octets stand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Octets {
    /// All kept, so far.
    Kept,
    /// More than the scratch holds: refused at the end, never cut.
    Overflow,
    /// A byte that is no base64: the file is not one.
    NotBase64,
}

/// How many octets of a file are in the scratch, from its start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileLen(usize);

impl FileLen {
    pub fn index(self) -> usize {
        self.0
    }
}

/// What a finished file's octets came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileOutcome {
    /// All of them, at the scratch's start.
    Whole,
    /// More than the scratch holds.
    Overflow,
    /// Its data is not base64.
    NotBase64,
}

/// **A finished file**: its arguments, what its octets came to, and how
/// much of the scratch they wrote, which goes back once the file is done
/// with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileEnd {
    pub args: FileArgs,
    pub outcome: FileOutcome,
    pub len: FileLen,
}

/// **A file being received**: its arguments, the decoder's carry and how
/// many octets are in the scratch.
#[derive(Clone, Copy)]
pub struct ItermFile {
    open: Option<(FileArgs, FileParts)>,
    b64: Base64,
    len: usize,
    octets: Octets,
}

impl ItermFile {
    pub const NEW: ItermFile = ItermFile {
        open: None,
        b64: Base64::NEW,
        len: 0,
        octets: Octets::Kept,
    };

    /// A file begins; one open before it is abandoned, and the return is
    /// the scratch it had written, as [`ItermFile::abandon`] gives it.
    pub fn begin(&mut self, args: FileArgs, parts: FileParts) -> Option<FileLen> {
        let was = self.abandon();
        self.open = Some((args, parts));
        was
    }

    /// Abandon the file open, if any -- a kitty transmission took the
    /// scratch, or a whole file's message was cancelled before its
    /// terminator -- returning the scratch its octets wrote.
    pub fn abandon(&mut self) -> Option<FileLen> {
        let was = self.open.map(|_| FileLen(self.len));
        *self = ItermFile::NEW;
        was
    }

    /// Which kind of file is open.
    pub fn parts(&self) -> Option<FileParts> {
        self.open.map(|(_, p)| p)
    }

    /// Whether the open file keeps its octets in the scratch: an inline one.
    pub fn holds_scratch(&self) -> bool {
        matches!(self.open, Some((a, _)) if a.shown == FileShown::Inline)
    }

    /// One base64 byte.
    pub fn put(&mut self, b: u8, scratch: &mut [u8]) {
        if self.open.is_none() || self.octets != Octets::Kept {
            return;
        }
        match self.b64.push(b) {
            Base64Step::Octets(o) => self.keep(o.as_slice(), scratch),
            Base64Step::Need => {}
            Base64Step::Invalid => self.octets = Octets::NotBase64,
        }
    }

    /// A run of base64, as [`ItermFile::put`] on each byte would take it.
    pub fn put_run(&mut self, mut run: &[u8], scratch: &mut [u8]) {
        let mut out = [0u8; 768];
        while !run.is_empty() {
            if self.open.is_none() || self.octets != Octets::Kept {
                return;
            }
            let before = self.b64;
            match self.b64.feed(run, &mut out) {
                Ok(fed) => {
                    self.keep(&out[..fed.wrote], scratch);
                    run = &run[fed.read..];
                }
                Err(_) => {
                    // A byte that is no base64 somewhere in the run: the
                    // byte path decides where, as kitty's run does.
                    self.b64 = before;
                    for &b in run {
                        self.put(b, scratch);
                    }
                    return;
                }
            }
        }
    }

    /// Octets into the scratch, an inline file's; a download's are counted
    /// and dropped.
    fn keep(&mut self, o: &[u8], scratch: &mut [u8]) {
        if !self.holds_scratch() {
            return;
        }
        match scratch.get_mut(self.len..self.len + o.len()) {
            Some(d) => {
                d.copy_from_slice(o);
                self.len += o.len();
            }
            None => self.octets = Octets::Overflow,
        }
    }

    /// **The file ends**: the decoder's carry finished into the octets and
    /// what they came to. The base64 of every part is one stream, finished
    /// here and nowhere before: iTerm2 appends each part's base64 to one
    /// string and decodes it at the end (`appendBase64EncodedData:`,
    /// `[[source: iterm2-964a75d-inline-image-helper]]`), so a part may be
    /// any length, as tmux's 256-byte limit makes them. `None` with none
    /// open.
    pub fn end(&mut self, scratch: &mut [u8]) -> Option<FileEnd> {
        let (args, _) = self.open?;
        if self.octets == Octets::Kept {
            match core::mem::replace(&mut self.b64, Base64::NEW).finish() {
                Ok(o) => self.keep(o.as_slice(), scratch),
                Err(_) => self.octets = Octets::NotBase64,
            }
        }
        let outcome = match self.octets {
            Octets::Kept => FileOutcome::Whole,
            Octets::Overflow => FileOutcome::Overflow,
            Octets::NotBase64 => FileOutcome::NotBase64,
        };
        let end = FileEnd {
            args,
            outcome,
            len: FileLen(self.len),
        };
        *self = ItermFile::NEW;
        Some(end)
    }
}

/// **The size of the broken-image placeholder along an axis given as
/// `auto`**: iTerm2's `broken_image` picture, 35 by 36 pixels, drawn as any
/// picture is, one pixel a device pixel (Jev's lean, ICON_SIZE, 0.54/0.52
/// and 0.80/0.72 asked again).
pub const BROKEN_W: u32 = 35;
pub const BROKEN_H: u32 = 36;

/// **Where an inline picture goes**: the cells it takes from the cursor, and
/// the device pixels it is drawn over from the first cell's top-left, which
/// may be fewer than the cells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileGeometry {
    pub cols: Cols,
    pub rows: Rows,
    pub draw_w: u32,
    pub draw_h: u32,
    /// The pixels across that are on the screen, the drawn width cut at its
    /// right edge, as iTerm2 writes no cell past it.
    pub shown_w: u32,
}

impl FileGeometry {
    /// The image the shown part is stored as, or `None` past what an image
    /// side holds.
    pub fn shown(&self) -> Option<(ImagePx, ImagePx)> {
        Some((
            ImagePx::of(u16::try_from(self.shown_w).ok()?),
            ImagePx::of(u16::try_from(self.draw_h).ok()?),
        ))
    }

    /// Whether `p` is drawn as it is, one pixel a device pixel and whole.
    pub fn draws_as_is(&self, p: Picture) -> bool {
        (self.draw_w, self.draw_h, self.shown_w) == (p.width, p.height, p.width)
    }
}

/// The screen and cell a picture is placed into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileScreen {
    pub cols: Cols,
    pub rows: Rows,
    pub cursor: Col,
    /// The cell, in device pixels.
    pub cell_w: ImagePx,
    pub cell_h: ImagePx,
}

/// A decoded picture: the image it is stored as, and its pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Picture {
    pub id: ImageId,
    pub width: u32,
    pub height: u32,
}

/// **The picture in the scratch's first `len` bytes, decoded into a stored
/// image of its own size**, its work in the scratch after it. Which format
/// it is the bytes say, as `NSImage`'s `initWithData:` reads them
/// (`[[source: iterm2-964a75d-image-with-data]]`), not the file's `name`: a
/// JPEG by its `SOI` and the marker after it ([`Jpeg::sniff`]), else a PNG
/// by its signature, its RGBA8 rows turned into straight ARGB words where
/// they lie, as kitty's decode turns them. `None`, the image freed, for
/// bytes that are neither, one that does not decode, or one the store
/// refuses.
///
/// The second result is how much of the scratch the decode wrote, the file
/// and its work, which go back once it is done.
pub fn decode_picture(
    store: &mut ImageStore,
    scratch: &mut [u8],
    len: FileLen,
) -> (Option<Picture>, FileLen) {
    let at = len.0.min(scratch.len());
    if Jpeg::sniff(&scratch[..at]) {
        let work = Jpeg::parse(&scratch[..at])
            .ok()
            .and_then(|j| j.scratch_len())
            .unwrap_or(0);
        let used = FileLen(at.saturating_add(work).min(scratch.len()));
        return (jpeg_of(store, scratch, at), used);
    }
    let raw = Png::parse(&scratch[..at])
        .ok()
        .and_then(|p| p.raw_len())
        .unwrap_or(0);
    let used = FileLen(at.saturating_add(raw).min(scratch.len()));
    (picture_of(store, scratch, at), used)
}

/// **The JPEG in the scratch's first `at` bytes**, decoded at the size it
/// is shown, its orientation applied, into a stored image; its work in the
/// scratch after it.
fn jpeg_of(store: &mut ImageStore, scratch: &mut [u8], at: usize) -> Option<Picture> {
    let (data, rest) = scratch.split_at_mut(at);
    let jpeg = Jpeg::parse(data).ok()?;
    let size = jpeg.size();
    let (w, h) = (
        u16::try_from(size.width).ok()?,
        u16::try_from(size.height).ok()?,
    );
    let (id, px) = store.store(ImagePx::of(w), ImagePx::of(h), ImageAlpha::OpaqueOrClear)?;
    match jpeg.decode(rest, px) {
        Ok(_) => Some(Picture {
            id,
            width: size.width,
            height: size.height,
        }),
        Err(_) => {
            store.free(id);
            None
        }
    }
}

fn picture_of(store: &mut ImageStore, scratch: &mut [u8], at: usize) -> Option<Picture> {
    let (data, rest) = scratch.split_at_mut(at);
    let png = Png::parse(data).ok()?;
    let size = png.size();
    let (w, h) = (
        u16::try_from(size.width).ok()?,
        u16::try_from(size.height).ok()?,
    );
    let (id, px) = store.store(ImagePx::of(w), ImagePx::of(h), ImageAlpha::OpaqueOrClear)?;
    match png.decode(rest, Pixel::bytes_mut(px)) {
        Ok(_) => {
            for p in px.iter_mut() {
                *p = p.from_memory_rgba();
            }
            Some(Picture {
                id,
                width: size.width,
                height: size.height,
            })
        }
        Err(_) => {
            store.free(id);
            None
        }
    }
}

/// **The broken-image placeholder into `px`**, the shown part of a
/// `g.draw_w` by `g.draw_h` box: a one-pixel frame round the box and its
/// two diagonals in `ink`, clear inside (Jev's lean, FRAME_CROSS,
/// 0.36/0.50, then split asked again, FRAME_CROSS 0.04 in one order and
/// SMALL_ICON 0.20 in the other, so the higher mean across the asks). iTerm2
/// draws its own `broken_image` picture there, which baud's licence cannot
/// take.
pub fn draw_placeholder(px: &mut [Pixel], g: &FileGeometry, ink: Pixel) {
    let (w, h, fw) = (g.shown_w as usize, g.draw_h as usize, g.draw_w as usize);
    px.fill(Pixel::CLEAR);
    if w == 0 || h == 0 || fw == 0 {
        return;
    }
    let mut set = |x: usize, y: usize| {
        if x < w && y < h {
            px[y * w + x] = ink;
        }
    };
    // Only the shown part is walked: the drawn width is the program's to
    // ask, and may be millions of pixels past the screen's edge.
    for x in 0..w {
        set(x, 0);
        set(x, h - 1);
    }
    for y in 0..h {
        set(0, y);
        set(fw - 1, y);
    }
    // Each diagonal, one pixel a step along the longer side, the steps on
    // the shown columns alone: step `i` lands at `i * (fw - 1) / (steps -
    // 1)` across, so the falling one is shown up to `w` and the rising one
    // from `fw - w`.
    let steps = fw.max(h) as u64;
    let span = (steps - 1).max(1);
    let (fw1, h1) = ((fw - 1) as u64, (h - 1) as u64);
    let step_at = |x: u64| (x * span).div_ceil(fw1.max(1));
    let falling = 0..(step_at(w as u64).min(steps));
    let rising = step_at((fw - w.min(fw)) as u64)
        .saturating_sub(1)
        .min(steps)..steps;
    for i in falling.chain(rising) {
        let x = (i * fw1 / span) as usize;
        let y = (i * h1 / span) as usize;
        set(x, y);
        set(fw - 1 - x, y);
    }
}

/// **iTerm2's arithmetic for an inline picture of `iw` by `ih` pixels**, in
/// device pixels (`VT100InlineImageHelper`, `[[source:
/// iterm2-964a75d-inline-image-helper]]`): with Retina inline images on, its
/// default, a picture's points are its pixels over the backing scale, and
/// `Npx` is N pixels over it too, so the scale cancels and every quantity
/// here is the device pixels it divides. In order:
///
/// * each axis, "N" cells, "Npx" `ceil(N / cell)` cells and N pixels, "N%"
///   `ceil(screen * N / 100)` cells and that share of the screen's pixels
///   (N clamped to 0-100), `auto` the picture's own pixels -- or, the other
///   axis given, the height from the width by the picture's aspect, and the
///   width from the height, rounded, after it;
/// * at least one cell each way;
/// * wider than the columns right of the cursor: the width set to the whole
///   screen and the height scaled by those columns over the width, as
///   iTerm2's source does (Jev, ITERM_FULL_WIDTH, 0.92/0.90), the pixels
///   then the cells;
/// * taller than 255 rows, iTerm2's own limit: 255, the width multiplied by
///   the rows over 255, as its source does (Jev, ITERM_CAP_LITERAL,
///   0.98/0.98);
/// * the picture drawn from the top-left of its cells, at the size asked,
///   fitted into it with its aspect kept, or stretched to it.
///
/// `None` while the cell size is unknown, or for a picture of no pixels.
pub fn file_geometry(args: FileArgs, iw: u32, ih: u32, s: FileScreen) -> Option<FileGeometry> {
    use core::f64::math::{ceil, round, trunc};
    if s.cell_w.raw() == 0 || s.cell_h.raw() == 0 || iw == 0 || ih == 0 {
        return None;
    }
    let (cw, ch) = (f64::from(s.cell_w.raw()), f64::from(s.cell_h.raw()));
    let (iw, ih) = (f64::from(iw), f64::from(ih));
    let grid_w = s.cols.index() as f64;
    let grid_h = s.rows.index() as f64;
    let pct = |n: i64| (n.clamp(0, 100) as f64) / 100.0;
    // The width, or `None` when it waits for the height.
    let (mut width, mut req_w, needs_width) = match args.width {
        FileExtent::Pixels(n) => (ceil(n as f64 / cw), n as f64, false),
        FileExtent::Percent(n) => (ceil(grid_w * pct(n)), grid_w * cw * pct(n), false),
        FileExtent::Cells(n) => (n as f64, n as f64 * cw, false),
        FileExtent::Auto => match args.height {
            FileExtent::Auto => (ceil(iw / cw), iw, false),
            _ => (0.0, 0.0, true),
        },
    };
    let (mut height, mut req_h) = match args.height {
        FileExtent::Pixels(n) => (ceil(n as f64 / ch), n as f64),
        FileExtent::Percent(n) => (ceil(grid_h * pct(n)), grid_h * ch * pct(n)),
        FileExtent::Cells(n) => (n as f64, n as f64 * ch),
        FileExtent::Auto => match args.width {
            FileExtent::Auto => (ceil(ih / ch), ih),
            _ => {
                let h = req_w / (iw / ih);
                (ceil(h / ch), h)
            }
        },
    };
    if needs_width {
        req_w = req_h * (iw / ih);
        width = round(req_w / cw);
    }
    width = width.max(1.0);
    height = height.max(1.0);
    let max_w = grid_w - s.cursor.index() as f64;
    if width > max_w {
        let scale = max_w / width;
        width = grid_w;
        // `*heightPtr *= scale` on an `int`: truncated.
        height = trunc(height * scale).max(1.0);
        req_w = width * cw;
        req_h = height * ch;
    }
    if height > 255.0 {
        let scale = height / 255.0;
        height = 255.0;
        width = trunc(width * scale).max(1.0);
        req_w = width * cw;
        req_h = height * ch;
    }
    let (dw, dh) = match args.aspect {
        FileAspect::Stretched => (req_w, req_h),
        FileAspect::Kept => {
            // iTerm2's `iTermImageInfoGetSizeForRegionPreservingAspectRatio`.
            let (image_ar, box_ar) = (iw / ih, req_w / req_h);
            match image_ar > box_ar {
                true => (req_w, req_w / image_ar),
                false => (req_h * image_ar, req_h),
            }
        }
    };
    let px = |v: f64| (round(v) as u32).max(1);
    let room = (s.cols.index().saturating_sub(s.cursor.index()) as u32) * u32::from(s.cell_w.raw());
    Some(FileGeometry {
        cols: Cols::of_index(width as usize),
        rows: Rows::of_count(height as usize),
        draw_w: px(dw),
        draw_h: px(dh),
        shown_w: px(dw).min(room),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> FileScreen {
        FileScreen {
            cols: Cols::of_index(80),
            rows: Rows::of_count(24),
            cursor: Col::ZERO,
            cell_w: ImagePx::of(10),
            cell_h: ImagePx::of(20),
        }
    }

    #[test]
    fn file_args_read_as_iterm2_reads_them() {
        let a = FileArgs::parse(
            b"name=eA==;size=9;width=50%;height=10px;inline=1;preserveAspectRatio=0",
        );
        assert_eq!(a.width, FileExtent::Percent(50));
        assert_eq!(a.height, FileExtent::Pixels(10));
        assert_eq!(a.shown, FileShown::Inline);
        assert_eq!(a.aspect, FileAspect::Stretched);
        let d = FileArgs::parse(b"");
        assert_eq!(d, FileArgs::DEFAULT);
        assert_eq!(FileArgs::parse(b"inline=yes").shown, FileShown::Inline);
        assert_eq!(FileArgs::parse(b"inline=0").shown, FileShown::Download);
        assert_eq!(FileArgs::parse(b"width=3").width, FileExtent::Cells(3));
        assert_eq!(FileArgs::parse(b"width=auto").width, FileExtent::Auto);
    }

    #[test]
    fn auto_is_the_pictures_own_pixels() {
        let g = file_geometry(FileArgs::DEFAULT, 35, 41, screen()).unwrap();
        assert_eq!((g.cols.index(), g.rows.index()), (4, 3));
        assert_eq!((g.draw_w, g.draw_h), (35, 41));
    }

    #[test]
    fn a_width_in_cells_takes_the_height_from_the_aspect() {
        let mut a = FileArgs::DEFAULT;
        a.width = FileExtent::Cells(10);
        let g = file_geometry(a, 200, 100, screen()).unwrap();
        // 100 px wide, 50 px tall: three rows.
        assert_eq!((g.cols.index(), g.rows.index()), (10, 3));
        assert_eq!((g.draw_w, g.draw_h), (100, 50));
    }

    #[test]
    fn a_box_keeps_the_aspect_from_its_top_left_or_stretches() {
        let mut a = FileArgs::DEFAULT;
        a.width = FileExtent::Cells(10);
        a.height = FileExtent::Cells(5);
        let g = file_geometry(a, 200, 100, screen()).unwrap();
        assert_eq!((g.cols.index(), g.rows.index()), (10, 5));
        assert_eq!((g.draw_w, g.draw_h), (100, 50), "fitted to the width");
        a.aspect = FileAspect::Stretched;
        let g = file_geometry(a, 200, 100, screen()).unwrap();
        assert_eq!((g.draw_w, g.draw_h), (100, 100));
    }

    #[test]
    fn too_wide_takes_the_whole_screen_as_iterm2_does() {
        let mut a = FileArgs::DEFAULT;
        a.width = FileExtent::Cells(100);
        a.height = FileExtent::Cells(10);
        let mut s = screen();
        s.cursor = Col::at_index(20);
        let g = file_geometry(a, 100, 100, s).unwrap();
        // 60 columns left of 100: the height 10 * 0.6, the width 80.
        assert_eq!((g.cols.index(), g.rows.index()), (80, 6));
    }

    #[test]
    fn taller_than_255_rows_is_capped_and_widened_as_iterm2_does() {
        let mut a = FileArgs::DEFAULT;
        a.width = FileExtent::Cells(2);
        a.height = FileExtent::Cells(510);
        let g = file_geometry(a, 10, 10, screen()).unwrap();
        assert_eq!((g.cols.index(), g.rows.index()), (4, 255));
    }

    /// **The placeholder walks the shown columns alone**, and draws there
    /// what walking the whole drawn width draws: the frame and both
    /// diagonals of a box ten times wider than the part shown.
    #[test]
    fn the_placeholder_draws_its_shown_part() {
        extern crate std;
        use std::vec;
        let ink = Pixel::opaque(
            crate::Chan::from_u8(9),
            crate::Chan::from_u8(8),
            crate::Chan::from_u8(7),
        );
        for (fw, w, h) in [
            (1000, 100, 37),
            (37, 37, 1000),
            (5, 5, 5),
            (1, 1, 9),
            (900, 37, 37),
        ] {
            let g = FileGeometry {
                cols: Cols::of_index(1),
                rows: Rows::of_count(1),
                draw_w: fw,
                draw_h: h,
                shown_w: w,
            };
            let mut got = vec![Pixel::CLEAR; (w * h) as usize];
            draw_placeholder(&mut got, &g, ink);
            // The whole width walked, the shown part kept.
            let (fwu, wu, hu) = (fw as usize, w as usize, h as usize);
            let mut want = vec![Pixel::CLEAR; wu * hu];
            let mut set = |x: usize, y: usize| {
                if x < wu && y < hu {
                    want[y * wu + x] = ink;
                }
            };
            for x in 0..fwu {
                set(x, 0);
                set(x, hu - 1);
            }
            for y in 0..hu {
                set(0, y);
                set(fwu - 1, y);
            }
            let steps = fwu.max(hu);
            for i in 0..steps {
                let x = i * (fwu - 1) / (steps - 1).max(1);
                let y = i * (hu - 1) / (steps - 1).max(1);
                set(x, y);
                set(fwu - 1 - x, y);
            }
            assert!(got == want, "{fw} wide, {w} shown, {h} tall");
        }
    }

    #[test]
    fn percent_and_pixels() {
        let mut a = FileArgs::DEFAULT;
        a.width = FileExtent::Percent(50);
        a.height = FileExtent::Pixels(45);
        a.aspect = FileAspect::Stretched;
        let g = file_geometry(a, 10, 10, screen()).unwrap();
        assert_eq!((g.cols.index(), g.rows.index()), (40, 3));
        assert_eq!((g.draw_w, g.draw_h), (400, 45));
        assert_eq!(
            file_geometry(
                a,
                10,
                10,
                FileScreen {
                    cell_w: ImagePx::ZERO,
                    ..screen()
                }
            ),
            None
        );
    }
}
