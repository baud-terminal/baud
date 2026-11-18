//! **baud's printer**: a print job, `[[source: dec-std-070]]` 070-7's print
//! stream in UTF-8, read as a DEC printer reads it and laid out in pages for
//! the system's print panel.
//!
//! What it acts on is the subset 070-7 § 7.4 sends and a controller run's
//! text commonly carries: `CR`, `LF`, `VT`, `BS`, `HT` to every eighth
//! column and `FF`, a new page, `LF`, `VT` and `FF` keeping the column, "the
//! corresponding character position" (`[[source: ecma48]]` 8.3.74, 8.3.161,
//! 8.3.51); `SGR` 0, 1, 4, 5 and 7 and their resets 22, 24, 25 and 27, bold,
//! underline, blink and reverse (blink drawn plain, as paper cannot blink);
//! `ESC # 3`, `4`, `5` and `6`, the line sizes. Every other control, escape
//! sequence and control string prints nothing and is counted in
//! [`Stripped`].
//!
//! **A character the head comes back over is struck over, not replaced**:
//! after a `CR` or a `BS` the next character prints on top of the one
//! there, as a printer's head strikes both and as `[[source: ecma48]]`
//! A.2 has it, "= BS /" processed "in such a way that it is preserved and
//! can be forwarded to a device which can indeed produce the intended
//! composite symbol" (6.4.2: "Composite graphic characters may be obtained
//! by using formator functions only"). A space adds no ink, so it strikes
//! nothing unless it is reversed or underlined.
//!
//! **A sixel dump prints its picture** (`[[source: dec-std-070]]` 070-9):
//! `DCS P1 ; P2 ; P3 q ... ST` is decoded as baud's terminal decodes a
//! Sixel, an undefined register inked black, the printer's "default Sixel
//! color" contrasting with white paper, and put on a line of its own where
//! the head is, each pixel the grid size square: `P3` in the units of the
//! last `SSU` (`CSI Ps SP I`; decipoints, 1/720 inch, until one is sent),
//! or with `P3` zero or absent the grid `P1` names (070-9 § 7.1-§ 7.3). The
//! background `P2` selects is "no action" on a hardcopy device (§ 7.2). A
//! picture wider or taller than the sheet is shrunk to fit it, its shape
//! kept. The head goes on at the start of the line after it. **A job's
//! pictures hold at most the [`PictureBudget`] the app sets**, all of them
//! together, since the size a dump names is the host's: one larger than
//! what is left is decoded at every k-th pixel each way, each pixel k times
//! larger so its size on paper is kept, and one with none left is left
//! out; both are counted in [`Stripped`].
//!
//! **A double-height line pair is printed once**: the `ESC # 3` line at
//! double width and height over two lines' space, and an `ESC # 4` line
//! straight after it not again, since a screen sends the same text on both
//! halves; a lone `ESC # 4` line is drawn as the top half is.

use baud_domain::{
    AmbiguousWidth, Chan, Codepoint, Pixel, Sixel, SixelAspect, SixelCanvas, SixelPx, Width,
};

/// Bold, underline, blink and reverse, as 070-7 § 7.4.2 sends them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Look(u8);

impl Look {
    pub const PLAIN: Look = Look(0);
    pub const BOLD: Look = Look(1);
    pub const UNDERLINE: Look = Look(2);
    pub const BLINK: Look = Look(4);
    pub const REVERSE: Look = Look(8);

    pub fn has(self, o: Look) -> bool {
        self.0 & o.0 == o.0
    }

    pub const fn with(self, o: Look) -> Look {
        Look(self.0 | o.0)
    }

    pub const fn without(self, o: Look) -> Look {
        Look(self.0 & !o.0)
    }
}

/// A line's size: `ESC # 5`, `6`, `3` and `4`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scale {
    Single,
    DoubleWidth,
    DoubleTop,
    DoubleBottom,
}

impl Scale {
    /// How many lines' height the line takes on paper.
    pub fn lines(self) -> Lines {
        match self {
            Scale::Single | Scale::DoubleWidth => Lines(1),
            Scale::DoubleTop | Scale::DoubleBottom => Lines(2),
        }
    }
}

/// A count of lines on paper.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Lines(usize);

impl Lines {
    /// How tall they are at `layout`.
    pub fn points(self, layout: Layout) -> Points {
        Points(f64::from(u32::try_from(self.0).unwrap_or(u32::MAX)) * layout.line_height)
    }
}

impl core::fmt::Display for Lines {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A count of columns: the widest line decides the type size.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Columns(usize);

impl Columns {
    /// How far across they go at `layout`.
    pub fn points(self, layout: Layout) -> Points {
        Points(f64::from(u32::try_from(self.0).unwrap_or(u32::MAX)) * layout.column)
    }

    /// As many spaces, what a picture's column is indented by.
    pub fn spaces(self) -> String {
        " ".repeat(self.0)
    }
}

impl core::fmt::Display for Columns {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One strike of the head: a character and its marks, in a look.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Struck {
    pub text: String,
    pub look: Look,
}

impl Struck {
    /// Whether it puts ink on paper: anything but a space, and a space
    /// reversed or underlined.
    fn inks(&self) -> bool {
        self.text != " " || self.look.has(Look::REVERSE) || self.look.has(Look::UNDERLINE)
    }

    /// The columns it takes.
    pub(crate) fn width(&self) -> Columns {
        let w = self.text.chars().next().map_or(1, |c| {
            Width::of(Codepoint::from_char(c), AmbiguousWidth::Narrow).cells()
        });
        Columns(w.max(1))
    }
}

/// Whether a column is where a character starts, or the second column of a
/// wide one the column before began.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Half {
    Lead,
    Tail,
}

/// One column of a line: what the head struck there, first struck first,
/// none for a column nothing inked.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cell {
    pub strikes: Vec<Struck>,
    pub half: Half,
}

impl Cell {
    const BLANK: Cell = Cell {
        strikes: Vec::new(),
        half: Half::Lead,
    };
}

/// A distance on paper, in points.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Points(pub f64);

/// A run of characters in one look, and how many columns the head goes
/// back after its last one, so that what follows is struck over it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Run {
    pub text: String,
    pub look: Look,
    pub back: Columns,
}

impl Run {
    pub fn new(text: &str, look: Look) -> Run {
        Run {
            text: text.into(),
            look,
            back: Columns(0),
        }
    }

    /// **How far its last character moves the next one back**, on a line
    /// of `scale` laid out at `layout`: its columns at `layout.column`
    /// each, twice that on a double-width or double-height line, which is
    /// drawn twice as wide. Zero for a run nothing is struck over.
    pub fn kern(&self, layout: Layout, scale: Scale) -> Points {
        let wide = match scale {
            Scale::Single => 1.0,
            Scale::DoubleWidth | Scale::DoubleTop | Scale::DoubleBottom => 2.0,
        };
        let back = f64::from(u32::try_from(self.back.0).unwrap_or(u32::MAX));
        Points(-back * layout.column * wide)
    }
}

/// A picture a sixel dump printed: its pixels, row by row, a pixel's side
/// on paper, and the column its upper left is at.
#[derive(Clone, PartialEq, Debug)]
pub struct Picture {
    pub width: SixelPx,
    pub height: SixelPx,
    pub pixels: Vec<Pixel>,
    pub dot: Points,
    pub column: Columns,
}

impl Picture {
    /// **Its size on paper at `layout`**: each pixel `dot` square, shrunk
    /// to fit the sheet's width less its column, and its height, with its
    /// shape kept.
    pub fn size(&self, layout: Layout) -> (Points, Points) {
        let px = |v: SixelPx| f64::from(v.raw());
        let (w, h) = (px(self.width) * self.dot.0, px(self.height) * self.dot.0);
        let room = (layout.width - self.column.points(layout).0).max(1.0);
        let shrink = (room / w.max(f64::MIN_POSITIVE))
            .min(layout.height / h.max(f64::MIN_POSITIVE))
            .min(1.0);
        (Points(w * shrink), Points(h * shrink))
    }

    /// The lines it takes at `layout`, at least one.
    pub fn lines(&self, layout: Layout) -> Lines {
        let h = self.size(layout).1.0;
        let fit = (1usize..=100_000)
            .take_while(|&n| {
                f64::from(u32::try_from(n - 1).unwrap_or(u32::MAX)) * layout.line_height < h
            })
            .count();
        Lines(fit.max(1))
    }
}

/// One printed line: text, or a picture.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    pub scale: Scale,
    pub cells: Vec<Cell>,
    pub picture: Option<Picture>,
}

impl Line {
    fn new() -> Line {
        Line {
            scale: Scale::Single,
            cells: Vec::new(),
            picture: None,
        }
    }

    /// The lines it takes on paper at `layout`: a picture's, or its size's.
    pub fn height(&self, layout: Layout) -> Lines {
        match &self.picture {
            Some(p) => p.lines(layout),
            None => self.scale.lines(),
        }
    }

    /// **The line's text in runs of one look**, trailing blank columns left
    /// off. The pen is followed across the line: a strike that starts left
    /// of where the one before left it has the run before go back the
    /// difference, so it is drawn over what is there, and one that starts
    /// right of it is reached through blanks. A narrow strike over a wide
    /// character's first column leaves the pen at its second, so what is
    /// struck there is not moved back.
    pub fn runs(&self) -> Vec<Run> {
        let mut out: Vec<Run> = Vec::new();
        let mut pen = 0usize;
        for (at, c) in self.cells.iter().enumerate() {
            for s in &c.strikes {
                match pen.checked_sub(at) {
                    Some(0) => {}
                    Some(back) => {
                        if let Some(r) = out.last_mut() {
                            r.back = Columns(back);
                        }
                    }
                    None => {
                        let blanks = " ".repeat(at - pen);
                        match out.last_mut() {
                            Some(r) if r.look == Look::PLAIN && r.back.0 == 0 => {
                                r.text.push_str(&blanks)
                            }
                            _ => out.push(Run::new(&blanks, Look::PLAIN)),
                        }
                    }
                }
                match out.last_mut() {
                    Some(r) if r.look == s.look && r.back.0 == 0 => r.text.push_str(&s.text),
                    _ => out.push(Run::new(&s.text, s.look)),
                }
                pen = at + s.width().0;
            }
        }
        out
    }

    /// The columns it takes, a double-width line twice its cells.
    pub fn columns(&self) -> Columns {
        match self.scale {
            Scale::Single => Columns(self.cells.len()),
            _ => Columns(self.cells.len() * 2),
        }
    }
}

/// One sheet of paper's lines.
#[derive(Clone, PartialEq, Debug)]
pub struct Page {
    pub lines: Vec<Line>,
}

/// What did not print as sent: controls, escape sequences and control
/// strings the printer does not act on, and the pictures reduced to fit the
/// job's [`PictureBudget`] or, with none of it left, left out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stripped {
    controls: Controls,
    reduced: Pictures,
    left_out: Pictures,
}

impl Stripped {
    /// Pictures printed at a fraction of their pixels.
    pub fn reduced(self) -> Pictures {
        self.reduced
    }

    /// Pictures not printed.
    pub fn left_out(self) -> Pictures {
        self.left_out
    }
}

impl core::fmt::Display for Stripped {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.controls.0)
    }
}

/// A count of controls, escape sequences and control strings.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Controls(usize);

/// A count of pictures.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pictures(pub usize);

impl Pictures {
    pub const NONE: Pictures = Pictures(0);
}

impl core::fmt::Display for Pictures {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// **The most pixels a job's pictures may hold**, all of them together:
/// the printer allocates each picture's pixels, four bytes each, from the
/// size the job names, which a host chooses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PictureBudget(pub usize);

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    EscHash,
    EscInter,
    Csi,
    Str,
    StrEsc,
    /// A `DCS`'s parameters, before its final.
    Dcs,
    /// A Sixel's body.
    Sixel,
    SixelEsc,
}

/// The unit `SSU` selects, in points: `[[source: ecma48]]` 8.3.139's
/// millimetre, computer decipoint (1/720 inch), mil, basic measuring unit
/// (1/1200 inch), micrometre and decipoint (0.03514 mm); `None` for the
/// character, the dididot and the pixel, which have no length here.
fn ssu_points(ps: &str) -> Option<f64> {
    match ps.parse::<u16>().unwrap_or(0) {
        1 => Some(72.0 / 25.4),
        2 => Some(0.1),
        4 => Some(0.072),
        5 => Some(0.06),
        6 => Some(72.0 / 25_400.0),
        8 => Some(0.035_14 * 72.0 / 25.4),
        _ => None,
    }
}

/// The horizontal grid 070-9 § 7.1's macro parameter names, in thousandths
/// of an inch, by its value 0 to 9: 7.5 for 0, 1 and 5, then 3, 4.5, 6, 9,
/// 10.5, 12 and 13.5 for 2 to 4 and 6 to 9; past 9 as the default.
const MACRO_GRID_MILS: [f64; 10] = [7.5, 7.5, 3.0, 4.5, 6.0, 7.5, 9.0, 10.5, 12.0, 13.5];

/// The printer's head: the forms so far, the line and column it is at, the
/// look it prints in, and the unit `SSU` selected, in points.
struct Head {
    forms: Vec<Vec<Line>>,
    line: Lines,
    col: Columns,
    look: Look,
    stripped: Stripped,
    unit: f64,
    /// What is left of the job's picture budget.
    budget: PictureBudget,
}

impl Head {
    fn lines(&mut self) -> &mut Vec<Line> {
        if self.forms.is_empty() {
            self.forms.push(Vec::new());
        }
        let form = self.forms.last_mut().expect("a form");
        while form.len() <= self.line.0 {
            form.push(Line::new());
        }
        form
    }

    fn put(&mut self, c: char) {
        let (line, col, look) = (self.line.0, self.col.0, self.look);
        let lines = self.lines();
        let cells = &mut lines[line].cells;
        let w = Width::of(Codepoint::from_char(c), AmbiguousWidth::Narrow).cells();
        if w == 0 {
            // A mark joins the character struck last before it.
            let before = col.min(cells.len());
            if let Some(prev) = cells[..before]
                .iter_mut()
                .rev()
                .find_map(|c| c.strikes.last_mut())
            {
                prev.text.push(c);
            }
            return;
        }
        while cells.len() < col + w {
            cells.push(Cell::BLANK);
        }
        let strike = Struck {
            text: c.into(),
            look,
        };
        if strike.inks() {
            cells[col].strikes.push(strike);
        }
        if w == 2 {
            cells[col + 1].half = Half::Tail;
        }
        self.col.0 += w;
    }

    fn control(&mut self, c: char) {
        match c {
            '\r' => self.col = Columns(0),
            // The line is made when something is printed on it or its size
            // is set, so a job's last `LF` leaves no empty line behind it.
            '\n' | '\u{b}' => self.line.0 += 1,
            '\u{c}' => {
                self.forms.push(Vec::new());
                self.line = Lines(0);
            }
            '\u{8}' => self.col.0 = self.col.0.saturating_sub(1),
            '\t' => self.col.0 = (self.col.0 / 8 + 1) * 8,
            _ => self.stripped.controls.0 += 1,
        }
    }

    /// **A sixel dump's picture** on a line of its own where the head is,
    /// the head going on at the start of the line after it.
    fn picture(&mut self, params: &str, body: &[u8]) {
        let mut ps = params.split(';').map(|p| p.parse::<u16>().ok());
        let p1 = ps.next().flatten();
        let p3 = ps.nth(1).flatten().unwrap_or(0);
        let aspect = SixelAspect::from_macro(p1);
        // The size the raster attributes give, "the number of pixels in an
        // image" each way (070-9 § 8.1.2), or with none what the data
        // reaches.
        let ext = Sixel::extent(body, aspect);
        let raster: Vec<u16> = match body.first() {
            Some(b'"') => body[1..]
                .split(|b| !(b.is_ascii_digit() || *b == b';'))
                .next()
                .unwrap_or(&[])
                .split(|&b| b == b';')
                .map(|n| {
                    core::str::from_utf8(n)
                        .ok()
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0)
                })
                .collect(),
            _ => Vec::new(),
        };
        let at = |i: usize| SixelPx::of(raster.get(i).copied().unwrap_or(0));
        let (w, h) = match (at(2), at(3)) {
            (w, h) if w > SixelPx::ZERO && h > SixelPx::ZERO => (w, h),
            _ => (ext.width, ext.height),
        };
        // **Held to what is left of the budget**: every `by`-th pixel each
        // way, `by` the least that fits, and none when nothing is left.
        // The sheet shows it shrunk to fit anyway, a picture this large
        // far more.
        let left = self.budget.0;
        if left == 0 && w.index() * h.index() > 0 {
            self.stripped.left_out.0 += 1;
            return;
        }
        let fits = |by: usize| w.index().div_ceil(by) * h.index().div_ceil(by) <= left;
        // No `by` below the square root of the ratio fits, so the search
        // starts there.
        let least = (w.index() * h.index() / left.max(1)).isqrt().max(1);
        let Some(by) = (least..=usize::from(u16::MAX)).find(|&by| fits(by)) else {
            self.stripped.left_out.0 += 1;
            return;
        };
        if by > 1 {
            self.stripped.reduced.0 += 1;
        }
        let part = |v: SixelPx| SixelPx::of(u16::try_from(v.index().div_ceil(by)).unwrap_or(0));
        let (w, h) = (part(w), part(h));
        self.budget.0 -= w.index() * h.index();
        let mut pixels = vec![Pixel::CLEAR; w.index() * h.index()];
        let ink = Pixel::opaque(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(0));
        let by16 = u16::try_from(by).unwrap_or(u16::MAX);
        Sixel::decode_inked(
            body,
            aspect,
            ink,
            &mut SixelCanvas::shrunk(&mut pixels, w, by16),
        );
        let scale = f64::from(by16);
        let dot = scale
            * match p3 {
                0 => {
                    let i = usize::from(p1.unwrap_or(0));
                    MACRO_GRID_MILS.get(i).copied().unwrap_or(7.5) * 0.072
                }
                n => f64::from(n) * self.unit,
            };
        let line = self.line.0;
        let at = match self.lines()[line].cells.is_empty() {
            true => line,
            false => line + 1,
        };
        self.line = Lines(at);
        let column = self.col;
        let lines = self.lines();
        lines[at].picture = Some(Picture {
            width: w,
            height: h,
            pixels,
            dot: Points(dot),
            column,
        });
        self.line = Lines(at + 1);
        self.col = Columns(0);
    }

    fn line_size(&mut self, c: char) {
        let scale = match c {
            '3' => Scale::DoubleTop,
            '4' => Scale::DoubleBottom,
            '5' => Scale::Single,
            '6' => Scale::DoubleWidth,
            _ => {
                self.stripped.controls.0 += 1;
                return;
            }
        };
        let line = self.line.0;
        self.lines()[line].scale = scale;
    }

    /// `SGR`, its parameters as sent: the four attributes and their resets;
    /// a colour's own parameters are passed over so none is read as an
    /// attribute.
    fn sgr(&mut self, params: &str) {
        let mut ps = params.split(';').map(|p| {
            let mut sub = p.split(':');
            let head = sub.next().unwrap_or("");
            let rest: Vec<&str> = sub.collect();
            (head.parse::<u16>().unwrap_or(0), rest)
        });
        while let Some((p, sub)) = ps.next() {
            self.look = match p {
                0 => Look::PLAIN,
                1 => self.look.with(Look::BOLD),
                4 if sub.first() == Some(&"0") => self.look.without(Look::UNDERLINE),
                4 | 21 => self.look.with(Look::UNDERLINE),
                5 | 6 => self.look.with(Look::BLINK),
                7 => self.look.with(Look::REVERSE),
                22 => self.look.without(Look::BOLD),
                24 => self.look.without(Look::UNDERLINE),
                25 => self.look.without(Look::BLINK),
                27 => self.look.without(Look::REVERSE),
                38 | 48 | 58 if sub.is_empty() => {
                    match ps.next().map(|n| n.0) {
                        Some(5) => drop(ps.next()),
                        Some(2) => {
                            ps.next();
                            ps.next();
                            ps.next();
                        }
                        _ => {}
                    }
                    continue;
                }
                _ => continue,
            };
        }
    }
}

/// **A job read into forms**, a form being what an `FF` ends; a form with
/// nothing on it after the last `FF` (DECPFF's) is no form.
pub fn read(job: &[u8], budget: PictureBudget) -> (Vec<Vec<Line>>, Stripped) {
    let text = String::from_utf8_lossy(job);
    let mut head = Head {
        forms: Vec::new(),
        line: Lines(0),
        col: Columns(0),
        look: Look::PLAIN,
        stripped: Stripped::default(),
        unit: 0.1,
        budget,
    };
    let mut state = State::Ground;
    let mut params = String::new();
    let mut csi_plain = true;
    let mut csi_space = false;
    let mut body: Vec<u8> = Vec::new();
    for c in text.chars() {
        state = match (state, c) {
            (State::Ground, '\u{1b}') => State::Esc,
            (State::Ground, '\u{9b}') => {
                params.clear();
                csi_plain = true;
                csi_space = false;
                State::Csi
            }
            (State::Ground, '\u{90}') => {
                params.clear();
                State::Dcs
            }
            (State::Ground, '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}') => State::Str,
            (State::Ground, c) if c.is_control() => {
                head.control(c);
                State::Ground
            }
            (State::Ground, c) => {
                head.put(c);
                State::Ground
            }
            (State::Esc, '[') => {
                params.clear();
                csi_plain = true;
                csi_space = false;
                State::Csi
            }
            (State::Esc, '#') => State::EscHash,
            (State::Esc, 'P') => {
                params.clear();
                State::Dcs
            }
            (State::Esc, 'X' | ']' | '^' | '_') => State::Str,
            (State::Esc, '\u{20}'..='\u{2f}') => State::EscInter,
            (State::Esc | State::EscInter, _) => {
                head.stripped.controls.0 += 1;
                State::Ground
            }
            (State::EscHash, c) => {
                head.line_size(c);
                State::Ground
            }
            (State::Csi, '0'..='9' | ';' | ':') => {
                params.push(c);
                State::Csi
            }
            (State::Csi, ' ') if csi_plain => {
                csi_space = true;
                csi_plain = false;
                State::Csi
            }
            (State::Csi, '\u{20}'..='\u{3f}') => {
                csi_plain = false;
                csi_space = false;
                State::Csi
            }
            (State::Csi, 'm') if csi_plain => {
                head.sgr(&params);
                State::Ground
            }
            (State::Csi, 'I') if csi_space => {
                match ssu_points(&params) {
                    Some(u) => head.unit = u,
                    None => head.stripped.controls.0 += 1,
                }
                State::Ground
            }
            (State::Csi, _) => {
                head.stripped.controls.0 += 1;
                State::Ground
            }
            (State::Str, '\u{7}' | '\u{9c}') => {
                head.stripped.controls.0 += 1;
                State::Ground
            }
            (State::Str, '\u{1b}') => State::StrEsc,
            (State::Str, _) => State::Str,
            (State::StrEsc, '\\') => {
                head.stripped.controls.0 += 1;
                State::Ground
            }
            (State::StrEsc, _) => State::Str,
            (State::Dcs, '0'..='9' | ';') => {
                params.push(c);
                State::Dcs
            }
            (State::Dcs, 'q') => {
                body.clear();
                State::Sixel
            }
            (State::Dcs, '\u{1b}') => State::StrEsc,
            (State::Dcs, _) => State::Str,
            (State::Sixel, '\u{1b}') => State::SixelEsc,
            (State::Sixel, '\u{9c}') | (State::SixelEsc, '\\') => {
                head.picture(&params, &body);
                State::Ground
            }
            (State::Sixel, c) => {
                if c.is_ascii() {
                    body.push(u8::try_from(u32::from(c)).unwrap_or(b'?'));
                }
                State::Sixel
            }
            // An `ESC` that is not `ST` cuts the dump off: what came is
            // printed, and the escape is counted.
            (State::SixelEsc, _) => {
                head.picture(&params, &body);
                head.stripped.controls.0 += 1;
                State::Ground
            }
        };
    }
    let mut forms = head.forms;
    if forms.last().is_some_and(|f| {
        f.iter()
            .all(|l| l.picture.is_none() && l.cells.iter().all(|c| c.strikes.is_empty()))
    }) {
        forms.pop();
    }
    (forms, head.stripped)
}

/// The widest line of the forms, in columns.
pub fn widest(forms: &[Vec<Line>]) -> Columns {
    forms
        .iter()
        .flatten()
        .map(Line::columns)
        .max()
        .unwrap_or(Columns(0))
}

/// **The forms on sheets of the lines `layout` holds**: a form starts a
/// sheet, a sheet that fills goes on to the next, a double-height line
/// takes two lines and a picture the lines its height does, each going
/// whole to the next sheet if one is left, and the bottom half of a
/// double-height pair is not printed again.
pub fn paginate(forms: &[Vec<Line>], layout: Layout) -> Vec<Page> {
    let per = layout.lines.0.max(2);
    let mut pages = Vec::new();
    for form in forms {
        let mut page = Vec::new();
        let mut used = 0;
        let mut after_top = false;
        for line in form {
            if line.scale == Scale::DoubleBottom && after_top {
                after_top = false;
                continue;
            }
            after_top = line.scale == Scale::DoubleTop;
            let h = line.height(layout).0;
            if used + h > per {
                pages.push(Page {
                    lines: core::mem::take(&mut page),
                });
                used = 0;
            }
            page.push(line.clone());
            used += h;
        }
        pages.push(Page { lines: page });
    }
    pages
}

/// The type size and line height a job prints at, and the lines a sheet
/// holds: ten points, smaller where the widest line would not fit across
/// the sheet, never under four. The font's advance and line height per
/// point are the system font's, measured by the caller.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Layout {
    pub size: f64,
    pub line_height: f64,
    /// A column's width at `size`, what a run going back moves by.
    pub column: f64,
    pub lines: Lines,
    /// The sheet's printable width and height, what a picture fits.
    pub width: f64,
    pub height: f64,
}

impl Layout {
    pub fn fit(width: f64, height: f64, advance: f64, leading: f64, widest: Columns) -> Layout {
        let cols = widest.0.max(1) as f64;
        let size = (width / (cols * advance)).clamp(4.0, 10.0);
        let line_height = size * leading;
        // The whole lines that fit, counted rather than cast.
        let fit = (1u32..=100_000)
            .take_while(|&n| f64::from(n) * line_height <= height)
            .count();
        let lines = Lines(fit.max(2));
        Layout {
            size,
            line_height,
            column: size * advance,
            lines,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job read with room for a 1024 by 1024 picture.
    fn read(job: &[u8]) -> (Vec<Vec<Line>>, Stripped) {
        super::read(job, PictureBudget(1 << 20))
    }

    /// Each column's last strike, a blank a space and a wide character's
    /// second column nothing.
    fn text(l: &Line) -> String {
        l.cells
            .iter()
            .map(|c| match (c.half, c.strikes.last()) {
                (_, Some(s)) => s.text.as_str(),
                (Half::Tail, None) => "",
                (Half::Lead, None) => " ",
            })
            .collect()
    }

    fn run(t: &str, look: Look, back: Columns) -> Run {
        Run {
            text: t.into(),
            look,
            back,
        }
    }

    /// **The core's print page reads back as the screen it printed**: line
    /// sizes, the four attributes, `CR LF` a line, and DECPFF's closing
    /// `FF` no empty sheet.
    #[test]
    fn a_print_page_reads_back_as_its_lines() {
        let job = b"\x1b[0m\x1b#5\x1b[0;1mB\x1b[0;4;7mU\x1b[0mx\r\n\x1b#6wide\r\n\x0c\x1b[0m";
        let (forms, stripped) = read(job);
        assert_eq!(stripped, Stripped::default());
        assert_eq!(forms.len(), 1, "the closing FF makes no sheet");
        let lines = &forms[0];
        assert_eq!(lines.len(), 2, "the last LF leaves no empty line");
        assert_eq!(
            lines[0].runs(),
            [
                Run::new("B", Look::BOLD),
                Run::new("U", Look::UNDERLINE.with(Look::REVERSE)),
                Run::new("x", Look::PLAIN),
            ]
        );
        assert_eq!(
            (lines[1].scale, text(&lines[1])),
            (Scale::DoubleWidth, "wide".into())
        );
        assert_eq!(lines[1].columns(), Columns(8));
    }

    /// `CR` and `BS` go back over a line, `HT` to the next eighth column,
    /// `LF` down a line and `FF` to a new sheet, each keeping the column
    /// (`[[source: ecma48]]` 8.3.74 and 8.3.51, "the corresponding character
    /// position"); a wide character takes two columns and a mark
    /// joins the character before it.
    #[test]
    fn the_format_effectors_move_the_head() {
        let (forms, _) = read("abc\rX\tY\u{8}Z\n!\r  \u{4e2d}e\u{301}\u{c}next".as_bytes());
        assert_eq!(forms.len(), 2);
        assert_eq!(text(&forms[0][0]), "Xbc     Z");
        assert_eq!(text(&forms[0][1]), "  \u{4e2d}e\u{301}    !");
        assert_eq!(
            forms[0][1].cells[3].half,
            Half::Tail,
            "the wide character's second column"
        );
        assert_eq!(text(&forms[1][0]), "     next", "FF keeps the column");
    }

    /// **A colour's parameters are passed over**, so `38;2;1;4;7` sets no
    /// attribute; `4:0` and the resets turn attributes off.
    #[test]
    fn sgr_takes_the_four_attributes_and_passes_colours_over() {
        let (forms, _) =
            read(b"\x1b[38;2;1;4;7ma\x1b[1;4:3;5;7mb\x1b[22;24;25;27mc\x1b[4m\x1b[4:0md");
        let looks: Vec<Look> = forms[0][0]
            .cells
            .iter()
            .map(|c| c.strikes[0].look)
            .collect();
        let all = Look::BOLD
            .with(Look::UNDERLINE)
            .with(Look::BLINK)
            .with(Look::REVERSE);
        assert_eq!(looks, [Look::PLAIN, all, Look::PLAIN, Look::PLAIN]);
    }

    /// Every other control, escape sequence and control string prints
    /// nothing and is counted.
    #[test]
    fn what_the_printer_does_not_act_on_is_stripped_and_counted() {
        let (forms, stripped) =
            read(b"a\x07\x1b(B\x1b[2;3H\x1b]0;title\x07\x1bP1$r0m\x1b\\\x1b[?25lb\x1b#8");
        assert_eq!(text(&forms[0][0]), "ab");
        assert_eq!(
            stripped,
            Stripped {
                controls: Controls(7),
                ..Stripped::default()
            }
        );
    }

    /// **Sheets of `layout.lines` lines**, a form starting a sheet; a double-height
    /// pair is one line two high, its bottom half not printed again, and
    /// goes whole to the next sheet.
    #[test]
    fn forms_go_on_sheets_and_a_double_height_pair_is_printed_once() {
        let (forms, _) =
            read(b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\x0cA\r\n\x1b#3T\r\n\x1b#4T\r\n\x1b#4L");
        let three = Layout::fit(480.0, 36.0, 0.6, 1.2, Columns(80));
        assert_eq!(three.lines, Lines(3));
        let pages = paginate(&forms, three);
        let shape: Vec<Vec<String>> = pages
            .iter()
            .map(|p| p.lines.iter().map(text).collect())
            .collect();
        assert_eq!(
            shape,
            [
                vec!["1", "2", "3"],
                vec!["4", "5", "6"],
                vec!["7"],
                vec![" A", "T"],
                vec!["L"]
            ]
        );
        assert_eq!(pages[3].lines[1].scale, Scale::DoubleTop);
    }

    /// **Ten points unless the widest line would not fit**, never under
    /// four, and the whole lines a sheet holds at that size.
    #[test]
    fn the_layout_fits_the_widest_line_across_the_sheet() {
        let at = |cols| Layout::fit(480.0, 720.0, 0.6, 1.2, Columns(cols));
        assert_eq!(at(80).size, 10.0);
        assert_eq!(at(80).lines, Lines(60));
        assert_eq!(at(100).size, 8.0);
        assert_eq!(at(1000).size, 4.0);
        assert_eq!(widest(&read(b"ab\r\n\x1b#6abc").0), Columns(6));
    }

    /// **A character the head comes back over is struck over**: `BS` and
    /// `CR` bring the head back, and what it strikes there is printed on top
    /// of what was there, each strike in its own look (`[[source: ecma48]]`
    /// A.2's "= BS /"); a mark joins the strike before it.
    #[test]
    fn a_struck_over_character_keeps_both() {
        let (forms, _) = read("a\u{8}_b\rX\x1b[1m\u{8}\u{8}\u{8}+e\u{301}".as_bytes());
        let col0: Vec<(&str, Look)> = forms[0][0].cells[0]
            .strikes
            .iter()
            .map(|s| (s.text.as_str(), s.look))
            .collect();
        assert_eq!(
            col0,
            [
                ("a", Look::PLAIN),
                ("_", Look::PLAIN),
                ("X", Look::PLAIN),
                ("+", Look::BOLD)
            ]
        );
        assert_eq!(forms[0][0].cells[1].strikes[1].text, "e\u{301}");
        assert_eq!(
            forms[0][0].runs(),
            [
                run("a", Look::PLAIN, Columns(1)),
                run("_", Look::PLAIN, Columns(1)),
                run("X", Look::PLAIN, Columns(1)),
                run("+", Look::BOLD, Columns(0)),
                run("b", Look::PLAIN, Columns(1)),
                run("e\u{301}", Look::BOLD, Columns(0)),
            ]
        );
    }

    /// **A space strikes nothing unless reversed or underlined**, and a
    /// wide character's second column struck over sends the head back
    /// over the wide one.
    #[test]
    fn a_space_adds_no_ink_and_a_wide_tail_can_be_struck() {
        let (forms, _) = read("ab\r \x1b[4m \x1b[0m\r\u{4e2d}\u{8}|".as_bytes());
        let line = &forms[0][0];
        let texts: Vec<Vec<&str>> = line
            .cells
            .iter()
            .map(|c| c.strikes.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(texts, [vec!["a", "\u{4e2d}"], vec!["b", " ", "|"]]);
        assert_eq!(line.cells[1].half, Half::Tail);
        assert_eq!(
            line.runs(),
            [
                run("a", Look::PLAIN, Columns(1)),
                run("\u{4e2d}", Look::PLAIN, Columns(1)),
                run("b", Look::PLAIN, Columns(1)),
                run(" ", Look::UNDERLINE, Columns(1)),
                run("|", Look::PLAIN, Columns(0)),
            ]
        );
    }

    /// The column each glyph of `runs` is drawn at, as the printer draws
    /// them: each at the pen, the pen going on its columns, a run's back
    /// taking the pen back; a space is left out.
    fn drawn(runs: &[Run]) -> Vec<(char, usize)> {
        let mut pen = 0usize;
        let mut out = Vec::new();
        for r in runs {
            for c in r.text.chars() {
                let w = Width::of(Codepoint::from_char(c), AmbiguousWidth::Narrow).cells();
                if w > 0 && c != ' ' {
                    out.push((c, pen));
                }
                pen += w;
            }
            pen -= r.back.0;
        }
        out
    }

    /// **A narrow strike over a wide character's first column leaves the
    /// pen at its second**, so what is struck there and after it is drawn
    /// in its own column, and a second column nothing struck is a blank.
    #[test]
    fn a_narrow_strike_over_a_wide_character_keeps_the_columns() {
        let line = |job: &str| read(job.as_bytes()).0[0][0].clone();
        assert_eq!(
            drawn(&line("\u{4e2d}\rax").runs()),
            [('\u{4e2d}', 0), ('a', 0), ('x', 1)]
        );
        assert_eq!(
            drawn(&line("\u{4e2d}\u{6587}\r____").runs()),
            [
                ('\u{4e2d}', 0),
                ('_', 0),
                ('_', 1),
                ('\u{6587}', 2),
                ('_', 2),
                ('_', 3)
            ]
        );
        assert_eq!(
            drawn(&line("\u{4e2d}b\ra").runs()),
            [('\u{4e2d}', 0), ('a', 0), ('b', 2)]
        );
        assert_eq!(
            drawn(&line("a\u{8}_b").runs()),
            [('a', 0), ('_', 0), ('b', 1)]
        );
    }

    /// **A run going back moves the next glyph back its columns**, twice as
    /// far on a line drawn twice as wide.
    #[test]
    fn a_run_going_back_kerns_by_its_columns() {
        let at = Layout::fit(480.0, 720.0, 0.6, 1.2, Columns(80));
        assert_eq!(at.column, 6.0, "ten points at 0.6 a point");
        let r = run("a", Look::PLAIN, Columns(2));
        assert_eq!(r.kern(at, Scale::Single), Points(-12.0));
        assert_eq!(r.kern(at, Scale::DoubleWidth), Points(-24.0));
        assert_eq!(r.kern(at, Scale::DoubleTop), Points(-24.0));
        assert_eq!(
            Run::new("a", Look::PLAIN).kern(at, Scale::Single),
            Points(-0.0)
        );
    }

    /// A dump `ST`-terminated, as the terminal's hard copy writes it.
    fn red_dump(ssu: &str, p3: &str) -> Vec<u8> {
        format!("A\r\n{ssu}\x1bP0;1;{p3}q\"1;1;4;6#1;2;100;0;0#1!4~-\x1b\\B").into_bytes()
    }

    /// **A sixel dump prints its picture on a line of its own**: decoded as
    /// the terminal decodes a Sixel, the pixel the grid in the last SSU's
    /// unit (decipoints until one is sent), the head on the line after it.
    #[test]
    fn a_sixel_dump_prints_its_picture() {
        let (forms, stripped) = read(&red_dump("\x1b[2 I", "6"));
        assert_eq!(
            stripped,
            Stripped::default(),
            "the SSU and the dump are read"
        );
        let lines = &forms[0];
        assert_eq!(lines.len(), 3, "A, the picture, B");
        assert_eq!(text(&lines[0]), "A");
        let p = lines[1].picture.as_ref().expect("a picture");
        assert_eq!((p.width.raw(), p.height.raw()), (4, 6));
        let red = Pixel::opaque(Chan::from_u8(255), Chan::from_u8(0), Chan::from_u8(0));
        assert!(p.pixels.iter().all(|&px| px == red));
        assert!((p.dot.0 - 0.6).abs() < 1e-9, "six decipoints");
        assert_eq!(text(&lines[2]), "B");
        let (forms, _) = read(&red_dump("\x1b[1 I", "2"));
        let p = forms[0][1].picture.as_ref().unwrap();
        assert!(
            (p.dot.0 - 2.0 * 72.0 / 25.4).abs() < 1e-9,
            "two millimetres"
        );
        let (forms, _) = read(b"\x1bP9q~~\x1b\\");
        let p = forms[0][0].picture.as_ref().unwrap();
        assert!((p.dot.0 - 13.5 * 0.072).abs() < 1e-9, "macro 9's grid");
        assert_eq!(
            p.pixels[0],
            Pixel::opaque(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(0)),
            "inked black"
        );
    }

    /// **A picture fits the sheet, its shape kept, and takes the lines its
    /// height does**; one too tall for what is left of a sheet starts the
    /// next.
    #[test]
    fn a_picture_fits_the_sheet_and_takes_its_lines() {
        let at = Layout::fit(480.0, 120.0, 0.6, 1.2, Columns(80));
        let (forms, _) = read(b"\x1b[2 I\x1bP0;1;6q\"1;1;1000;100!1000~\x1b\\");
        let p = forms[0][0].picture.as_ref().unwrap();
        let (w, h) = p.size(at);
        assert!(
            (w.0 - 480.0).abs() < 1e-9 && (h.0 - 48.0).abs() < 1e-9,
            "{w:?} {h:?}"
        );
        let (forms, _) = read(b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n\x1b[2 I\x1bP0;1;10q\"1;1;40;48!40~-!40~-!40~-!40~-!40~-!40~-!40~-!40~\x1b\\");
        assert_eq!(forms[0][7].height(at), Lines(4), "48 points at 12 a line");
        let pages = paginate(&forms, at);
        assert_eq!(pages.len(), 2);
        assert!(
            pages[1].lines[0].picture.is_some(),
            "it goes whole to the next sheet"
        );
    }

    /// **A picture past the job's pixel budget is reduced to fit it**:
    /// every k-th pixel each way, k the least that fits, each k times
    /// larger on paper so its size is kept; the size the raster attributes
    /// name and the one the data reaches are both held. With none of the
    /// budget left a picture is left out. Both are counted.
    #[test]
    fn a_picture_past_the_budget_is_reduced_or_left_out() {
        let budget = PictureBudget(10_000);
        let black = Pixel::opaque(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(0));
        let (forms, stripped) =
            super::read(b"\x1b[2 I\x1bP0;1;6q\"1;1;65535;65535#1~\x1b\\", budget);
        let p = forms[0][0].picture.as_ref().expect("a picture");
        assert_eq!((p.width.raw(), p.height.raw()), (100, 100), "k = 656");
        assert_eq!(p.pixels.len(), 10_000);
        assert_eq!(p.pixels[0], black, "the sixel's six pixels in one");
        assert!((p.dot.0 - 0.6 * 656.0).abs() < 1e-9, "{:?}", p.dot);
        assert_eq!(
            (stripped.reduced(), stripped.left_out()),
            (Pictures(1), Pictures::NONE)
        );
        let (forms, _) = super::read(b"\x1bP0;1;6q\"1;1;1000;1000#1!500?~\x1b\\", budget);
        let p = forms[0][0].picture.as_ref().expect("a picture");
        assert_eq!(p.width.raw(), 100, "k = 10");
        let lit: Vec<usize> = (0..p.pixels.len())
            .filter(|&i| p.pixels[i] != Pixel::CLEAR)
            .collect();
        assert_eq!(lit, [50], "column 500, rows 0 to 5, in one pixel");
        let mut far = b"\x1bP0;1;6q".to_vec();
        far.extend(core::iter::repeat_n(b'-', 10_922));
        far.extend_from_slice(b"!65535~\x1b\\");
        let (forms, stripped) = super::read(&far, budget);
        let p = forms[0][0].picture.as_ref().expect("a picture");
        assert!(p.pixels.len() <= 10_000, "{}", p.pixels.len());
        assert_eq!(
            stripped.reduced(),
            Pictures(1),
            "the data's extent is held too"
        );
        let two = b"\x1bP0;1;6q\"1;1;100;100#1~\x1b\\\x1bP0;1;6q\"1;1;2;2#1~\x1b\\";
        let (forms, stripped) = super::read(two, budget);
        let p = forms[0][0].picture.as_ref().expect("the first, whole");
        assert_eq!((p.width.raw(), p.height.raw()), (100, 100));
        assert_eq!(forms[0].len(), 1, "the second left out");
        assert_eq!(
            (stripped.reduced(), stripped.left_out()),
            (Pictures::NONE, Pictures(1))
        );
    }
}
