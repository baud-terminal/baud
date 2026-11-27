//! **The graphics protocol's Unicode placeholders**
//! (`[[source: kitty-graphics]]`, "Unicode placeholders"): a U+10EEEE cell shows a slice of an image,
//! "encoding the image ID in its foreground color", the placement id in its
//! underline colour, and its row, its column and the id's most significant
//! byte in the diacritics after it, "listed in rowcolumn-diacritics.txt"
//! (`[[source: kitty-rowcolumn-diacritics]]`). What is missing is inherited
//! from the cell to the left.
//!
//! **What a cell holds is decided as it is written; what it shows is
//! resolved as it is drawn.** The diacritics of a placeholder are kept in
//! the cell's own 8-byte content word ([`Content::placeholder`]), not in
//! the cluster store: `kitten icat` writes three on every cell
//! (`[[source: kitty-icat-transmit]]`), every cell a different cluster, and
//! the 1024-slot store dropped 2366 of the 5760 marks of an 80x24 picture.
//! The inheritance reads the neighbours and the colours, which later writes
//! change, so it is resolved a row at a time by [`PlaceholderRuns`], when a
//! renderer asks.
//!
//! **The cell to the left is the adjacent one, and colours compare by the
//! ids they carry**: a run of cells continues while the ids from the
//! foreground and underline colours are equal and the row, column and byte
//! are absent or follow from the cell before, kitty's
//! `screen_render_line_graphics` (`[[source: kitty-screen-c]]`). Where the
//! text's three rules apply this is what they say; where none applies, a
//! cell takes row 0, column 0 and byte 0 for what it lacks, which the text
//! leaves open and kitty does. `38;5;42` and `38;2;0;0;42` both name id
//! 42, where the text's words compare "colors": a departure.

use crate::placeholder_data::{DIACRITIC_BLOCKS, DIACRITIC_INDEX, DIACRITIC_SHIFT, DIACRITICS};
use crate::{Codepoint, Col, ColorWord, Cols, KittyId, KittyPlacement, Rows};

/// The placeholder character, U+10EEEE: "a single Private Use Unicode
/// character as a placeholder to indicate to the terminal that an image is
/// supposed to be displayed at that cell".
pub const PLACEHOLDER: u32 = 0x10_EEEE;

/// Whether `c` is [`PLACEHOLDER`].
pub fn is_placeholder(c: Codepoint) -> bool {
    c.value() == PLACEHOLDER
}

/// **A diacritic's number**, `0..297`: the line of the pinned list it is
/// on, from zero.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Diacritic(u16);

impl Diacritic {
    /// How many the list holds.
    pub const COUNT: usize = DIACRITICS.len();

    /// The number `c` stands for, if it is on the list: two indexed loads.
    pub fn of(c: Codepoint) -> Option<Diacritic> {
        let page = usize::try_from(c.value() >> DIACRITIC_SHIFT).ok()?;
        let block = usize::from(*DIACRITIC_INDEX.get(page)?);
        let low = usize::try_from(c.value() & ((1 << DIACRITIC_SHIFT) - 1)).ok()?;
        let n = *DIACRITIC_BLOCKS.get((block << DIACRITIC_SHIFT) + low)?;
        n.checked_sub(1).map(Diacritic)
    }

    /// The diacritic for the number `n`, if there is one.
    pub fn at(n: usize) -> Option<Diacritic> {
        (n < Diacritic::COUNT).then(|| Diacritic(u16::try_from(n).unwrap_or(0)))
    }

    /// Its codepoint.
    pub fn codepoint(self) -> Codepoint {
        Codepoint(DIACRITICS[usize::from(self.0)])
    }

    /// Its number.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// **The diacritics after a placeholder, as written**: the row, then the
/// column, then the most significant byte of the image id, each absent
/// when fewer were written. What [`Content::placeholder`] keeps in a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlaceholderMarks {
    pub row: Option<Diacritic>,
    pub col: Option<Diacritic>,
    pub msb: Option<Diacritic>,
}

impl PlaceholderMarks {
    /// None written.
    pub const NONE: PlaceholderMarks = PlaceholderMarks {
        row: None,
        col: None,
        msb: None,
    };

    /// These and `d` after them, if there is room: three at most, and a
    /// later one only after the ones before it.
    pub fn then(self, d: Diacritic) -> Option<PlaceholderMarks> {
        match (self.row, self.col, self.msb) {
            (None, _, _) => Some(PlaceholderMarks {
                row: Some(d),
                ..self
            }),
            (Some(_), None, _) => Some(PlaceholderMarks {
                col: Some(d),
                ..self
            }),
            (Some(_), Some(_), None) => Some(PlaceholderMarks {
                msb: Some(d),
                ..self
            }),
            _ => None,
        }
    }

    /// How many were written.
    pub fn len(self) -> usize {
        [self.row, self.col, self.msb]
            .iter()
            .take_while(|d| d.is_some())
            .count()
    }

    /// Whether none was.
    pub fn is_empty(self) -> bool {
        self.row.is_none()
    }

    /// The `k`th written, as its codepoint.
    pub fn get(self, k: usize) -> Option<Codepoint> {
        [self.row, self.col, self.msb]
            .get(k)
            .copied()
            .flatten()
            .map(Diacritic::codepoint)
    }

    /// The diacritics a list of marks spells, as kitty reads them: the
    /// first three marks, each its number or absent when it is not on the
    /// list (`[[source: kitty-screen-c]]`). For a placeholder whose marks
    /// are kept as a cluster -- a fourth mark, or one not on the list.
    pub fn read(marks: impl Iterator<Item = u32>) -> PlaceholderMarks {
        let mut d = [None; 3];
        for (slot, m) in d.iter_mut().zip(marks) {
            *slot = Codepoint::new(m).and_then(Diacritic::of);
        }
        PlaceholderMarks {
            row: d[0],
            col: d[1],
            msb: d[2],
        }
    }
}

/// The id a colour carries: a 24-bit colour's value, a palette colour's
/// index, none for the default -- "8-bit IDs in 256 color mode or 24-bit
/// IDs in true color mode".
pub fn colour_id(c: ColorWord) -> u32 {
    match (c.as_rgb(), c.as_index()) {
        (Some((r, g, b)), _) => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        (None, Some(i)) => u32::from(i),
        (None, None) => 0,
    }
}

/// One cell of a row as the resolution reads it: a placeholder's
/// diacritics and the two colours that carry its ids, or not a placeholder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaceholderCell {
    Other,
    Placeholder {
        marks: PlaceholderMarks,
        fg: ColorWord,
        underline: ColorWord,
    },
}

/// **A run of placeholder cells that show one strip of one image**: from
/// column `at` of the screen, `cells` long, image row `row` from image
/// column `col`. The image id is the foreground's with the top byte; the
/// placement id the underline's, none when it is zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlaceholderRun {
    pub at: Col,
    pub cells: Cols,
    pub image: KittyId,
    pub placement: KittyPlacement,
    pub row: Rows,
    pub col: Cols,
}

/// What the cell before carried, resolved.
#[derive(Clone, Copy)]
struct Prev {
    id: u32,
    placement: u32,
    row: usize,
    col: usize,
    msb: usize,
}

/// **A row's placeholder cells resolved into runs**, left to right, the
/// inheritance of the module's rules applied as each cell is read.
pub struct PlaceholderRuns<I> {
    cells: I,
    x: usize,
    prev: Option<Prev>,
    run: Option<(usize, usize, Prev)>,
}

impl<I: Iterator<Item = PlaceholderCell>> PlaceholderRuns<I> {
    pub fn new(cells: I) -> PlaceholderRuns<I> {
        PlaceholderRuns {
            cells,
            x: 0,
            prev: None,
            run: None,
        }
    }

    /// The run `(start, length, first cell)` as a [`PlaceholderRun`].
    fn finish(run: (usize, usize, Prev)) -> PlaceholderRun {
        let (at, n, p) = run;
        // "The most significant byte": a number past 255 keeps its low
        // eight bits, as kitty's shift does.
        let msb = u32::try_from(p.msb & 0xff).unwrap_or(0);
        PlaceholderRun {
            at: Col(at),
            cells: Cols(n),
            image: KittyId::of(p.id | (msb << 24)),
            placement: KittyPlacement::of(p.placement),
            row: Rows(p.row),
            col: Cols(p.col),
        }
    }
}

impl<I: Iterator<Item = PlaceholderCell>> Iterator for PlaceholderRuns<I> {
    type Item = PlaceholderRun;

    fn next(&mut self) -> Option<PlaceholderRun> {
        loop {
            let Some(cell) = self.cells.next() else {
                return self.run.take().map(Self::finish);
            };
            let x = self.x;
            self.x += 1;
            let PlaceholderCell::Placeholder {
                marks,
                fg,
                underline,
            } = cell
            else {
                self.prev = None;
                if let Some(r) = self.run.take() {
                    return Some(Self::finish(r));
                }
                continue;
            };
            let (id, placement) = (colour_id(fg), colour_id(underline));
            let num = |d: Option<Diacritic>| d.map(Diacritic::index);
            let (row, col, msb) = (num(marks.row), num(marks.col), num(marks.msb));
            let follows = self.prev.filter(|p| {
                p.id == id
                    && p.placement == placement
                    && row.is_none_or(|r| r == p.row)
                    && col.is_none_or(|c| c == p.col + 1)
                    && msb.is_none_or(|m| m == p.msb)
            });
            let here = match follows {
                Some(p) => Prev {
                    col: p.col + 1,
                    ..p
                },
                None => Prev {
                    id,
                    placement,
                    row: row.unwrap_or(0),
                    col: col.unwrap_or(0),
                    msb: msb.unwrap_or(0),
                },
            };
            self.prev = Some(here);
            match (follows, self.run.take()) {
                (Some(_), Some((at, n, first))) => self.run = Some((at, n + 1, first)),
                (_, done) => {
                    self.run = Some((x, 1, here));
                    if let Some(r) = done {
                        return Some(Self::finish(r));
                    }
                }
            }
        }
    }
}
