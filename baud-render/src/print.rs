//! **baud's printer**: a print job, `[[source: dec-std-070]]` 070-7's print
//! stream in UTF-8, read as a DEC printer reads it and laid out in pages for
//! the system's print panel.
//!
//! What it acts on is the subset 070-7 § 7.4 sends and a controller run's
//! text commonly carries (Jev's lean, DEC_PRINTER, 0.66/0.70 then
//! 0.56/0.68): `CR`, `LF`, `VT`, `BS`, `HT` to every eighth column and `FF`,
//! a new page, `LF`, `VT` and `FF` keeping the column, "the corresponding
//! character position" (`[[source: ecma48]]` 8.3.74, 8.3.161, 8.3.51); `SGR` 0, 1, 4, 5 and 7 and their resets 22, 24, 25 and 27,
//! bold, underline, blink and reverse (blink drawn plain, as paper cannot
//! blink); `ESC # 3`, `4`, `5` and `6`, the line sizes. Every other control,
//! escape sequence and control string prints nothing and is counted in
//! [`Stripped`]. A character a `CR` or `BS` returns over replaces the one
//! there, where a printer's head would strike both.
//!
//! **A double-height line pair is printed once** (Jev's lean, DOUBLE_SIZE,
//! 0.70/0.72 then 0.76/0.78): the `ESC # 3` line at double width and height
//! over two lines' space, and an `ESC # 4` line straight after it not again,
//! since a screen sends the same text on both halves; a lone `ESC # 4` line
//! is drawn as the top half is.

use baud_domain::{AmbiguousWidth, Codepoint, Width};

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

impl core::fmt::Display for Lines {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A count of columns: the widest line decides the type size.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Columns(usize);

impl core::fmt::Display for Columns {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One column of a line: a character and its marks, or nothing for the
/// second column of a wide one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cell {
    pub text: String,
    pub look: Look,
}

/// One printed line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    pub scale: Scale,
    pub cells: Vec<Cell>,
}

impl Line {
    fn new() -> Line {
        Line {
            scale: Scale::Single,
            cells: Vec::new(),
        }
    }

    /// The line's text in runs of one look, trailing blanks of the plain
    /// look left off.
    pub fn runs(&self) -> Vec<(String, Look)> {
        let mut out: Vec<(String, Look)> = Vec::new();
        let end = self
            .cells
            .iter()
            .rposition(|c| !(c.text == " " && c.look == Look::PLAIN))
            .map_or(0, |i| i + 1);
        for c in &self.cells[..end] {
            match out.last_mut() {
                Some((t, l)) if *l == c.look => t.push_str(&c.text),
                _ => out.push((c.text.clone(), c.look)),
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
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Page {
    pub lines: Vec<Line>,
}

/// What printed nothing: controls, escape sequences and control strings
/// the printer does not act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stripped(usize);

impl core::fmt::Display for Stripped {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    EscHash,
    EscInter,
    Csi,
    Str,
    StrEsc,
}

/// The printer's head: the forms so far, the line and column it is at, the
/// look it prints in.
struct Head {
    forms: Vec<Vec<Line>>,
    line: Lines,
    col: Columns,
    look: Look,
    stripped: Stripped,
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
            // A mark joins the character before it.
            let before = col.min(cells.len());
            if let Some(prev) = cells[..before]
                .iter_mut()
                .rev()
                .find(|c| !c.text.is_empty())
            {
                prev.text.push(c);
            }
            return;
        }
        while cells.len() < col + w {
            cells.push(Cell {
                text: " ".into(),
                look: Look::PLAIN,
            });
        }
        cells[col] = Cell {
            text: c.into(),
            look,
        };
        if w == 2 {
            cells[col + 1] = Cell {
                text: String::new(),
                look,
            };
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
            _ => self.stripped.0 += 1,
        }
    }

    fn line_size(&mut self, c: char) {
        let scale = match c {
            '3' => Scale::DoubleTop,
            '4' => Scale::DoubleBottom,
            '5' => Scale::Single,
            '6' => Scale::DoubleWidth,
            _ => {
                self.stripped.0 += 1;
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
pub fn read(job: &[u8]) -> (Vec<Vec<Line>>, Stripped) {
    let text = String::from_utf8_lossy(job);
    let mut head = Head {
        forms: Vec::new(),
        line: Lines(0),
        col: Columns(0),
        look: Look::PLAIN,
        stripped: Stripped(0),
    };
    let mut state = State::Ground;
    let mut params = String::new();
    let mut csi_plain = true;
    for c in text.chars() {
        state = match (state, c) {
            (State::Ground, '\u{1b}') => State::Esc,
            (State::Ground, '\u{9b}') => {
                params.clear();
                csi_plain = true;
                State::Csi
            }
            (State::Ground, '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}') => State::Str,
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
                State::Csi
            }
            (State::Esc, '#') => State::EscHash,
            (State::Esc, 'P' | 'X' | ']' | '^' | '_') => State::Str,
            (State::Esc, '\u{20}'..='\u{2f}') => State::EscInter,
            (State::Esc | State::EscInter, _) => {
                head.stripped.0 += 1;
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
            (State::Csi, '\u{20}'..='\u{3f}') => {
                csi_plain = false;
                State::Csi
            }
            (State::Csi, 'm') if csi_plain => {
                head.sgr(&params);
                State::Ground
            }
            (State::Csi, _) => {
                head.stripped.0 += 1;
                State::Ground
            }
            (State::Str, '\u{7}' | '\u{9c}') => {
                head.stripped.0 += 1;
                State::Ground
            }
            (State::Str, '\u{1b}') => State::StrEsc,
            (State::Str, _) => State::Str,
            (State::StrEsc, '\\') => {
                head.stripped.0 += 1;
                State::Ground
            }
            (State::StrEsc, _) => State::Str,
        };
    }
    let mut forms = head.forms;
    if forms
        .last()
        .is_some_and(|f| f.iter().all(|l| l.cells.is_empty()))
    {
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

/// **The forms on sheets of `per` lines**: a form starts a sheet, a sheet
/// that fills goes on to the next, a double-height line takes two lines and
/// goes whole to the next sheet if one is left, and the bottom half of a
/// pair is not printed again.
pub fn paginate(forms: &[Vec<Line>], per: Lines) -> Vec<Page> {
    let per = per.0.max(2);
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
            let h = line.scale.lines().0;
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
    pub lines: Lines,
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
            lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Line) -> String {
        l.cells.iter().map(|c| c.text.as_str()).collect()
    }

    /// **The core's print page reads back as the screen it printed**: line
    /// sizes, the four attributes, `CR LF` a line, and DECPFF's closing
    /// `FF` no empty sheet.
    #[test]
    fn a_print_page_reads_back_as_its_lines() {
        let job = b"\x1b[0m\x1b#5\x1b[0;1mB\x1b[0;4;7mU\x1b[0mx\r\n\x1b#6wide\r\n\x0c\x1b[0m";
        let (forms, stripped) = read(job);
        assert_eq!(stripped, Stripped(0));
        assert_eq!(forms.len(), 1, "the closing FF makes no sheet");
        let lines = &forms[0];
        assert_eq!(lines.len(), 2, "the last LF leaves no empty line");
        assert_eq!(
            lines[0].runs(),
            [
                ("B".to_string(), Look::BOLD),
                ("U".to_string(), Look::UNDERLINE.with(Look::REVERSE)),
                ("x".to_string(), Look::PLAIN),
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
            forms[0][1].cells[3].text, "",
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
        let looks: Vec<Look> = forms[0][0].cells.iter().map(|c| c.look).collect();
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
            read(b"a\x07\x1b(B\x1b[2;3H\x1b]0;title\x07\x1bPq#0~\x1b\\\x1b[?25lb\x1b#8");
        assert_eq!(text(&forms[0][0]), "ab");
        assert_eq!(stripped, Stripped(7));
    }

    /// **Sheets of `per` lines**, a form starting a sheet; a double-height
    /// pair is one line two high, its bottom half not printed again, and
    /// goes whole to the next sheet.
    #[test]
    fn forms_go_on_sheets_and_a_double_height_pair_is_printed_once() {
        let (forms, _) =
            read(b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\x0cA\r\n\x1b#3T\r\n\x1b#4T\r\n\x1b#4L");
        let pages = paginate(&forms, Lines(3));
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
}
