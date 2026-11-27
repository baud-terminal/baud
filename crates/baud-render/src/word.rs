//! **A Devanagari word drawn whole.**
//!
//! While `?2027` is reset the core gives each letter and spacing vowel sign
//! a column of its own, as glibc and the Linux shells count, and with it set
//! a syllable takes one cell (`baud_domain::ClusterCells`). Either way the
//! face's letters are not the cells' width: shaped one syllable to its own
//! cells, a syllable wider than them was shrunk, and one narrower left its
//! headline open beside the next. So the renderer draws a **word** -- every
//! run of Devanagari cells between blanks, punctuation and anything else --
//! as text is set: shaped as one run at the face's natural advances, its
//! clusters laid end to end (`baud_font::Pens::Run`), each glyph at its own
//! sub-pixel pen, and centred in the word's columns. ECMA-48 lets a
//! character position's size "depend on the graphic symbol of the
//! character being imaged" (`[[source: ecma48]]` 6.3).
//!
//! **Where it does not fit** ([`lay`]), the wide-glyph rule at word
//! level: its ink may reach into the
//! blank cells beside it, keeping 0.26 em -- Noto Sans Devanagari's own
//! space -- from the next word's half of the blanks between them, and a
//! word space from anything else; the row's edge and a non-blank cell are
//! walls; the line's last word may take one blank cell after it. Then it is
//! shifted inside that room. Then it is drawn at the widest narrower
//! instance of its face that fits (`crate::stack::FaceRole::Narrower`, Noto's
//! `wdth` axis down to 62.5), at full height. **It is never scaled.** A word
//! that fits at no width (the residue) may close on the word beside it down
//! to half a word space and keep off a wall's cell; one that still does not
//! fit is kept inside the row and overlaps.
//!
//! **The cell is the data position, and its syllable's pen span the
//! presentation position** (`[[source: ecma48]]` 4.2.27, 4.2.6, 6.1.5): the
//! block cursor and the selection light the syllable that holds the cell,
//! a rectangle over its pen span under the glyphs, the glyph pieces inside
//! in the light's text colour ([`crate::renderer::Renderer::word_at`]).

use crate::cellmap::{BlockCursor, Selected, Text};
use crate::color::Argb;
use crate::fit::{Blank, Ink};
use crate::syllable::Syllable;
use baud_domain::TermRead;
use baud_domain::contrast::CurveRow;
use baud_domain::{Chan, Codepoint, Col, Cols, Content, GlyphId, PackedCell};

/// **Whether `c` is a letter of a word drawn whole**: a character of the
/// Devanagari block, its Extended block or the Vedic Extensions -- the
/// danda and the digits with them, as a run of the script is what a face
/// shapes as one.
pub(crate) fn is_letter(c: Codepoint) -> bool {
    let v = c.value();
    baud_domain::DEVANAGARI_RANGES
        .iter()
        .any(|&(a, z)| a <= v && v <= z)
}

/// Whether a row holds a Devanagari letter at all, which
/// [`crate::syllable::of_row`] notes as it walks: a row of none holds
/// no word, and is not walked again for one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Letters {
    None,
    Seen,
}

/// What stands past the blank cells beside a word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// Another word, whose ink shares the blanks between them in halves.
    Word,
    /// Any other cell that is not blank: a wall, kept a word space from.
    Wall,
    /// The row's edge.
    Edge,
}

/// **What a cell an input method's composition draws holds once the
/// commit is written** (`crate::overlay::Taken`): blank, a letter of a
/// word, or anything else -- what the row's words read it as, rather than
/// the blank the grid draws there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Held {
    Blank,
    Letter,
    Other,
}

/// How many blank cells stand beside a word: none, unlike a [`Cols`],
/// which is at least one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Blanks(pub usize);

/// The blank cells beside a word on each side, and what stands past them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Room {
    pub left: (Blanks, Side),
    pub right: (Blanks, Side),
}

/// **One word on a row**: its cells, from `at`, and its syllables left to
/// right, each the cells the core parted it into.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Word {
    pub at: Col,
    pub cells: Cols,
    pub syllables: Vec<Syllable>,
    pub room: Room,
}

impl Word {
    /// The column past its last cell.
    pub(crate) fn end(&self) -> Col {
        Col::at_index(self.at.index() + self.cells.index())
    }

    /// Whether column `c` is one of its cells.
    pub(crate) fn holds(&self, c: Col) -> bool {
        c >= self.at && c < self.end()
    }
}

/// **The words of `row`**, left to right as drawn: every run of cells whose
/// base is a [`is_letter`], each cell's text following the one left of it
/// -- `logical` gives the column in the text a drawn column shows, so on a
/// line drawn in reverse each cell is a word of its own. A syllable the core
/// parted (`parted`, [`crate::syllable::of_row`]'s for the row) is one of
/// the word's syllables, any other letter cell a syllable alone; one that
/// starts on a cell that is not a letter, a Latin letter with a Devanagari
/// vowel sign after it, is left to the syllable path whole. A cell `held`
/// answers for -- one an input method's composition draws, which the row
/// holds converted blank -- is in no word of the row's and is read as what
/// it will hold, so a word beside it is laid as it will be once the commit
/// is written; only a cell nothing was written in is asked.
pub(crate) fn of_row(
    term: &impl TermRead,
    row: &[PackedCell],
    logical: impl Fn(Col) -> Col,
    parted: &[Syllable],
    held: impl Fn(usize) -> Option<Held>,
) -> Vec<Word> {
    // A plain codepoint is read in place, so a row of ASCII costs one
    // compare a cell; a cluster's base is asked of the core.
    let letter = |x: usize| {
        row.get(x).is_some_and(|c| match c.content.as_codepoint() {
            Some(cp) => is_letter(cp),
            None => {
                !c.content.is_wide_tail()
                    && crate::renderer::drawn_base(term, *c).is_some_and(is_letter)
            }
        })
    };
    let blank = |x: usize| {
        row.get(x)
            .is_some_and(|c| match c.content == Content::EMPTY {
                true => held(x).is_none_or(|h| h == Held::Blank),
                false => Blank::of(term, *c) == Blank::Yes,
            })
    };
    let mut out = Vec::new();
    let mut k = 0usize;
    let mut x = 0usize;
    let parted_at = |k: &mut usize, x: usize| {
        while parted.get(*k).is_some_and(|s| s.at.index() < x) {
            *k += 1;
        }
        parted.get(*k).copied().filter(|s| s.at.index() == x)
    };
    while x < row.len() {
        if !letter(x) {
            // A syllable led by a cell that is not a letter is drawn whole by
            // the syllable path, its letters with it.
            x += parted_at(&mut k, x).map_or(1, |s| s.cells.index().max(1));
            continue;
        }
        let start = x;
        let mut syllables = Vec::new();
        loop {
            let s = parted_at(&mut k, x)
                .filter(|s| s.at.index() + s.cells.index() <= row.len())
                .unwrap_or(Syllable {
                    at: Col::at_index(x),
                    cells: Cols::of_index(1),
                });
            syllables.push(s);
            x += s.cells.index().max(1);
            let follows = |x: usize| {
                logical(Col::at_index(x)).index() == logical(Col::at_index(x - 1)).index() + 1
            };
            if x >= row.len() || !letter(x) || !follows(x) {
                break;
            }
        }
        let side_of = |x: Option<usize>| match x {
            None => Side::Edge,
            Some(x) if letter(x) => Side::Word,
            Some(x)
                if row.get(x).is_some_and(|c| c.content == Content::EMPTY)
                    && held(x) == Some(Held::Letter) =>
            {
                Side::Word
            }
            Some(_) => Side::Wall,
        };
        let mut l = start;
        while l > 0 && blank(l - 1) {
            l -= 1;
        }
        let mut r = x;
        while r < row.len() && blank(r) {
            r += 1;
        }
        out.push(Word {
            at: Col::at_index(start),
            cells: Cols::of_index(x - start),
            syllables,
            room: Room {
                left: (Blanks(start - l), side_of(l.checked_sub(1))),
                right: (Blanks(r - x), side_of((r < row.len()).then_some(r))),
            },
        });
    }
    out
}

/// **The face each cell of `w` is drawn in**, from its first: the face its
/// rendition names in `table`, the resolved table the frame draws with
/// ([`crate::cellmap::face_of`]). A syllable is shaped in its first cell's
/// face, as it takes its colours from that cell ([`WordPaint`]), and a
/// word whose syllables name two faces is shaped in runs cut where the
/// face changes (`crate::renderer::Renderer::word_at`): `[[source:
/// ecma48]]` 8.3.117 has SGR "establish one or more graphic rendition
/// aspects for subsequent text". Within a syllable each glyph is drawn
/// from its own cell's face where that face numbers its glyphs as the
/// syllable's does.
pub(crate) fn faces_of(
    table: &[crate::cellmap::Resolved],
    row: &[PackedCell],
    w: &Word,
) -> Vec<crate::TextFace> {
    (w.at.index()..w.end().index())
        .map(|c| crate::cellmap::face_of(table, row.get(c).copied().unwrap_or(PackedCell::EMPTY)))
        .collect()
}

/// **A word's text**: each syllable's characters, its cells' in order,
/// and how many of them each of the word's cells holds, from its first --
/// none for a wide character's tail -- so a syllable whose cells are drawn
/// from two faces is shaped with each cell its own cluster
/// (`crate::renderer::Renderer::word_at`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct WordText {
    pub parts: Vec<Vec<Codepoint>>,
    pub held: Vec<usize>,
}

/// **A word's measure on one instance**, in pixels from its pen's start:
/// its advance, from its first pen to the end of its last spacing glyph,
/// and where its ink lies.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Measure {
    pub advance: f32,
    pub ink: Ink,
}

/// How a word was placed, as [`lay`] tried it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum How {
    /// Centred in its columns, its ink inside them.
    Centred,
    /// Flush toward a cell written against it -- a comma, a hyphen -- its
    /// ink inside its columns.
    Flush,
    /// Centred, its ink into the blank cells beside it.
    Borrows,
    /// Moved from the centre to fit the room beside it.
    Shifted,
    /// Fits at no instance: closed on the word beside it, kept off walls.
    Residue,
    /// Fits at no instance even so: kept inside the row, overlapping.
    Over,
}

/// **Where a word goes**: the instance it is drawn at (0 the face itself,
/// then its narrower instances widest first), its pen's start in pixels
/// from its first cell's left edge, and how.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Laid {
    pub instance: usize,
    pub off: f32,
    pub how: How,
}

/// The cell grid a word is laid in: a cell's width, the gap kept between
/// words (0.26 em), and the row's width in cells.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Grid {
    pub cell: f32,
    pub gap: f32,
    pub cols: Cols,
}

/// A pixel's hundredth, so a word exactly as wide as its room fits.
const SLACK: f32 = 0.01;

/// Which rule a word's room is read by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    /// Rule D: half the blanks beside a word less half a word space, the
    /// blanks before a wall less a word space, at the line's end one cell.
    Shared,
    /// The residue: half the blanks beside a word less a quarter of a word
    /// space -- so two residues beside each other, or a residue and a word
    /// that took its whole half, keep half a word space -- every blank
    /// before a wall or the row's edge.
    Residue,
}

/// Which of a word's two sides a reach is read on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flank {
    /// The side toward the row's start.
    Start,
    /// The side toward the row's end.
    End,
}

/// How far a word's ink may reach past its cells on one side: `k` blank
/// cells, `side` past them, on `flank` of the word.
fn allowance((k, side): (Blanks, Side), flank: Flank, grid: Grid, rule: Rule) -> f32 {
    let room = grid.cell * k.0 as f32;
    match (side, rule) {
        (Side::Word, Rule::Shared) => ((room - grid.gap) / 2.0).max(0.0),
        (Side::Word, Rule::Residue) => ((room - grid.gap / 2.0) / 2.0).max(0.0),
        (Side::Wall, Rule::Shared) => (room - grid.gap).max(0.0),
        (Side::Wall, Rule::Residue) => room,
        (Side::Edge, Rule::Shared) if flank == Flank::End => room.min(grid.cell),
        (Side::Edge, _) => room,
    }
}

/// **Where a word's pen starts, from its first cell's edge, as it would
/// stand**: centred in its columns, or flush toward a cell written against
/// it on one side only -- a comma or a hyphen with no blank between, as
/// typeset text sets punctuation against its word -- its pen's end or
/// start on that cell's edge, the face's side bearing kept.
fn wanted(span: f32, advance: f32, room: Room) -> f32 {
    let against = |(k, side): (Blanks, Side)| k.0 == 0 && side == Side::Wall;
    match (against(room.left), against(room.right)) {
        (false, true) => span - advance,
        (true, false) => 0.0,
        _ => (span - advance) / 2.0,
    }
}

/// **Lays a word of `cells` columns from `at`** in its `room`: the widest
/// of `instances` instances -- `measure` reads each, widest first -- at
/// which it fits where it would stand ([`wanted`]), else shifted; at none,
/// the narrowest by the residue's rule -- toward the word before it down
/// to half a word space from `prev`, that word's ink's right edge in
/// pixels from this word's first cell's left edge, where known -- else
/// kept inside the row. Never scaled.
pub(crate) fn lay(
    (at, cells): (Col, Cols),
    room: Room,
    grid: Grid,
    (instances, prev): (usize, Option<f32>),
    mut measure: impl FnMut(usize) -> Option<Measure>,
) -> Laid {
    let span = grid.cell * cells.index() as f32;
    let fit = |m: Measure, rule: Rule| {
        let want = wanted(span, m.advance, room);
        let mut lo = -allowance(room.left, Flank::Start, grid, rule);
        // The residue closes on the word before it by that word's ink, down
        // to half a word space, and never into its cells.
        if let (Rule::Residue, (k, Side::Word), Some(p)) = (rule, room.left, prev) {
            lo = (p + grid.gap / 2.0).max(-grid.cell * k.0 as f32);
        }
        let hi = span + allowance(room.right, Flank::End, grid, rule);
        let (il, ir) = (m.ink.left, m.ink.right);
        if want + il >= lo - SLACK && want + ir <= hi + SLACK {
            let inside = want + il >= -SLACK && want + ir <= span + SLACK;
            return Some((want, inside));
        }
        (ir - il <= hi - lo + SLACK).then(|| (want.clamp(lo - il, (hi - ir).max(lo - il)), false))
    };
    let mut last = None;
    for j in 0..instances.max(1) {
        let Some(m) = measure(j) else {
            continue;
        };
        if let Some((off, inside)) = fit(m, Rule::Shared) {
            let want = wanted(span, m.advance, room);
            let centre = ((span - m.advance) / 2.0 - want).abs() <= SLACK;
            let how = match (inside, (off - want).abs() <= SLACK, centre) {
                (true, true, true) => How::Centred,
                (true, true, false) => How::Flush,
                (false, true, _) => How::Borrows,
                _ => How::Shifted,
            };
            return Laid {
                instance: j,
                off,
                how,
            };
        }
        last = Some((j, m));
    }
    let Some((j, m)) = last else {
        return Laid {
            instance: 0,
            off: 0.0,
            how: How::Over,
        };
    };
    if let Some((off, _)) = fit(m, Rule::Residue) {
        return Laid {
            instance: j,
            off,
            how: How::Residue,
        };
    }
    // Over even so: where it would stand, inside the row where it is
    // narrower than it.
    let want = wanted(span, m.advance, room);
    let row_lo = -grid.cell * at.index() as f32;
    let row_hi = grid.cell * grid.cols.index().saturating_sub(at.index()) as f32;
    let (il, ir) = (m.ink.left, m.ink.right);
    Laid {
        instance: j,
        off: want.clamp(row_lo - il, (row_hi - ir).max(row_lo - il)),
        how: How::Over,
    }
}

/// A syllable's place among its word's, left to right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SyllableIx(pub u16);

/// Whether a word drawn whole holds a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Worded {
    No,
    Yes,
}

/// **Where a word was drawn**, as the last convert laid it: its cells, the
/// instance it is drawn at (0 its face, then the narrower ones widest
/// first), how [`lay`] placed it, its ink's left and right edges, its
/// syllables, their pen spans and its glyphs' pens, in pixels of the
/// screen. What a test reads to
/// check the drawing against its rules.
#[derive(Clone, PartialEq, Debug)]
pub struct WordLaid {
    pub at: Col,
    pub cells: Cols,
    /// The blank cells before and after it, and whether a word, a wall or
    /// the row's edge stands past each.
    pub room: (usize, Side, usize, Side),
    pub instance: usize,
    pub how: How,
    pub ink: (f32, f32),
    /// Each syllable's first column and cells.
    pub syllables: Vec<(Col, Cols)>,
    /// Each syllable's pen span, rounded to the pixel: what its light lights.
    pub spans: Vec<(i32, i32)>,
    /// Each glyph and the pen it is drawn at, to a quarter of a pixel.
    pub pens: Vec<(GlyphId, f32)>,
    /// The face each glyph of `pens` is drawn from: its syllable's, its
    /// own cell's where that face numbers its glyphs alike, or a narrower
    /// instance of either.
    pub faces: Vec<crate::FaceIx>,
}

/// **What lights a cell**: the selection, the block cursor, and the
/// block's opacity this frame -- the swaps a cell's colours are drawn
/// with (`crate::cellmap::Swaps`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Light {
    pub selected: Selected,
    pub block: BlockCursor,
    pub cursor: Chan,
}

impl Light {
    /// Neither: the cell's own colours.
    pub(crate) const NONE: Light = Light {
        selected: Selected::No,
        block: BlockCursor::No,
        cursor: Chan::from_offset(0xFF, 0),
    };

    /// Lit by anything: the selection, or a block this frame draws some of
    /// -- a block blinked off, at no opacity, lights nothing.
    pub(crate) fn lit(self) -> bool {
        self.selected == Selected::Yes
            || (matches!(self.block, BlockCursor::Yes(_)) && self.cursor.get() > 0)
    }

    /// This light and `o`'s: a syllable is lit where any of its cells is.
    pub(crate) fn with(self, o: Light) -> Light {
        Light {
            selected: match (self.selected, o.selected) {
                (Selected::No, Selected::No) => Selected::No,
                _ => Selected::Yes,
            },
            block: match self.block {
                BlockCursor::No => o.block,
                b => b,
            },
            cursor: match self.block {
                BlockCursor::No => o.cursor,
                _ => self.cursor,
            },
        }
    }
}

/// **The colours a ground and the text on it are drawn in**: what a cell
/// draws, or a lit syllable's rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Colours {
    pub fg: Argb,
    pub bg: Argb,
    pub curve: CurveRow,
}

/// **One cell of a row as a word's ink sees it**: the colours it draws its
/// ground and text in -- a word's own cells unlit, their light drawn over
/// their syllable's span instead -- and what lights it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Under {
    pub colours: Colours,
    pub light: Light,
}

/// **Each syllable of a word as it is drawn**: its first cell's colours
/// under the light of every cell it holds, and whether its text draws.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct WordPaint {
    pub syllables: Vec<(Colours, Light, Text)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid {
            cell: 10.0,
            gap: 3.0,
            cols: Cols::of_index(80),
        }
    }

    fn m(advance: f32, left: f32, right: f32) -> Measure {
        Measure {
            advance,
            ink: Ink { left, right },
        }
    }

    fn room(l: (usize, Side), r: (usize, Side)) -> Room {
        Room {
            left: (Blanks(l.0), l.1),
            right: (Blanks(r.0), r.1),
        }
    }

    /// A word narrower than its columns is centred in them, whatever is
    /// beside it.
    #[test]
    fn a_word_that_fits_is_centred() {
        let l = lay(
            (Col::at_index(5), Cols::of_index(3)),
            room((0, Side::Wall), (0, Side::Wall)),
            grid(),
            (1, None),
            |_| Some(m(24.0, 1.0, 23.0)),
        );
        assert_eq!(l.how, How::Centred);
        assert!((l.off - 3.0).abs() < 1e-4, "{l:?}");
    }

    /// Wider than its columns, a word between two others takes half the
    /// blank cell beside it less half the gap, and no more: 3.5 px each
    /// side of a 10 px cell with a 3 px gap.
    #[test]
    fn a_wider_word_borrows_half_the_blanks_less_half_the_gap() {
        let both = room((1, Side::Word), (1, Side::Word));
        let fits = lay(
            (Col::at_index(5), Cols::of_index(2)),
            both,
            grid(),
            (1, None),
            |_| Some(m(27.0, 0.0, 27.0)),
        );
        assert_eq!(fits.how, How::Borrows);
        let over = lay(
            (Col::at_index(5), Cols::of_index(2)),
            both,
            grid(),
            (1, None),
            |_| Some(m(27.2, 0.0, 27.2)),
        );
        assert_ne!(over.how, How::Borrows, "{over:?}");
    }

    /// **Against a comma or a hyphen it is laid flush**: its pen's start on
    /// the cell written before it, or its end on the one after, the rest of
    /// its columns left over on the other side, as typeset text sets
    /// punctuation against its word; centred between two.
    #[test]
    fn against_punctuation_a_word_is_laid_flush() {
        let at = (Col::at_index(5), Cols::of_index(2));
        let lay_in = |r: Room| lay(at, r, grid(), (1, None), |_| Some(m(16.0, 1.0, 15.0)));
        let before = lay_in(room((0, Side::Wall), (2, Side::Edge)));
        assert_eq!((before.how, before.off), (How::Flush, 0.0), "{before:?}");
        let after = lay_in(room((1, Side::Word), (0, Side::Wall)));
        assert_eq!((after.how, after.off), (How::Flush, 4.0), "{after:?}");
        let both = lay_in(room((0, Side::Wall), (0, Side::Wall)));
        assert_eq!((both.how, both.off), (How::Centred, 2.0), "{both:?}");
    }

    /// At the row's edge it borrows nothing, so one wider than its columns
    /// is shifted toward the side with room.
    #[test]
    fn at_the_rows_edge_a_word_is_shifted_away_from_it() {
        let l = lay(
            (Col::at_index(0), Cols::of_index(2)),
            room((0, Side::Edge), (2, Side::Edge)),
            grid(),
            (1, None),
            |_| Some(m(30.0, 0.0, 30.0)),
        );
        assert_eq!(l.how, How::Shifted, "{l:?}");
        assert!(
            l.off.abs() < 1e-4,
            "its ink starts on its first cell: {l:?}"
        );
    }

    /// **It is narrowed before it is left over**: the widest instance that
    /// fits is taken, the wider ones skipped, and none is ever scaled.
    #[test]
    fn the_widest_instance_that_fits_is_taken() {
        let widths = [40.0, 34.0, 30.0, 26.0];
        let l = lay(
            (Col::at_index(5), Cols::of_index(3)),
            room((0, Side::Wall), (0, Side::Wall)),
            grid(),
            (widths.len(), None),
            |j| Some(m(widths[j], 0.0, widths[j])),
        );
        assert_eq!((l.instance, l.how), (2, How::Centred), "{l:?}");
    }

    /// **The residue closes on its neighbour and keeps off a wall's cell**:
    /// at no instance does it fit, so it takes the blanks before the wall
    /// and half the blanks toward the word less a quarter of the gap.
    #[test]
    fn the_residue_keeps_off_a_wall_and_closes_on_a_word() {
        let r = room((2, Side::Word), (0, Side::Wall));
        let l = lay(
            (Col::at_index(5), Cols::of_index(2)),
            r,
            grid(),
            (1, None),
            |_| Some(m(29.0, 0.0, 29.0)),
        );
        assert_eq!(l.how, How::Residue, "{l:?}");
        assert!(l.off + 29.0 <= 20.0 + SLACK, "off the wall's cell: {l:?}");
        assert!(
            l.off >= -(20.0 - 1.5) / 2.0 - SLACK,
            "half a gap kept: {l:?}"
        );
    }

    /// **The residue closes on the word before it by its ink**: where that
    /// word's ink ends 15 px before this one's cells, the residue takes all
    /// but half a word space of it, and fits where half the blanks would
    /// leave it over.
    #[test]
    fn the_residue_closes_on_the_ink_of_the_word_before_it() {
        let r = room((2, Side::Word), (0, Side::Wall));
        let at = (Col::at_index(5), Cols::of_index(2));
        let near = lay(at, r, grid(), (1, Some(-15.0)), |_| {
            Some(m(33.0, 0.0, 33.0))
        });
        assert_eq!(near.how, How::Residue, "{near:?}");
        assert!(
            near.off >= -15.0 + 1.5 - SLACK,
            "half a word space kept: {near:?}"
        );
        let blind = lay(at, r, grid(), (1, None), |_| Some(m(33.0, 0.0, 33.0)));
        assert_eq!(blind.how, How::Over, "{blind:?}");
    }

    /// Over even at the residue's room, a word is kept inside the row.
    #[test]
    fn a_word_over_every_room_stays_inside_the_row() {
        let l = lay(
            (Col::at_index(0), Cols::of_index(1)),
            room((0, Side::Edge), (0, Side::Wall)),
            grid(),
            (1, None),
            |_| Some(m(25.0, 0.0, 25.0)),
        );
        assert_eq!(l.how, How::Over);
        assert!(l.off >= -SLACK, "not past the row's left edge: {l:?}");
    }
}
