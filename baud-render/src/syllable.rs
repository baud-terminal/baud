//! **A syllable drawn across its cells.**
//!
//! While `?2027` is reset the core gives each legacy grapheme cluster a cell
//! of its own (`baud_domain::ClusterCells`), so र्मि -- RA, VIRAMA, MA, VOWEL
//! SIGN I -- is three cells, र्, म and ि, three columns as glibc and the
//! Linux shells count them. The shaper reorders and joins a syllable whole,
//! the vowel sign before the consonant it follows and the reph over it, so
//! the renderer puts the cells a syllable was parted into back together: it
//! runs the extended rules (`[[source: uax29]]`) across the row's cells and
//! joins a cell to the one before it where those rules join its first
//! character by GB9a, GB9b or GB9c -- the joins the legacy rules do not make,
//! read from the core's own generated table -- or joins a spacing mark UAX
//! #29 classes Extend, such as Tamil's ா, which the core gives a column too
//! (`ClusterBreak::parts_at`). A cell
//! that holds such a join inside it -- a whole syllable written while
//! `?2027` is set, or a vowel sign with spacing marks joined -- is drawn as
//! any cluster in one cell is, at its font's size.
//!
//! **A Devanagari syllable is drawn with its word** (`crate::word`): the
//! word shaped as one run at its face's natural advances and laid in its
//! columns, never scaled. **Any other syllable is shaped alone and drawn
//! across exactly its cells**, centred in them at its face's size, one
//! wider than them overlapping the cells beside it
//! ([`crate::renderer::Renderer::syllable`]). The instance path, which the
//! CPU path draws from, and the cell path's records draw both from the same
//! glyphs, and the cell path masks every one of their cells off the map.
//!
//! Only within a row: a syllable a soft wrap parts is drawn as two, each
//! row's part shaped alone, so a vowel sign carried to the next row is drawn
//! on the dotted circle the shaper gives a mark with no base. And only left
//! to right on the screen: a row is walked as drawn, and a cell joins the
//! one drawn left of it only where it follows it in the text too, so on a
//! line drawn in reverse each cell is drawn alone; a Devanagari run inside
//! a right-to-left paragraph keeps its order and its syllables.

use baud_core::Term;
use baud_domain::{ClusterBreak, ClusterCells, Codepoint, Col, Cols, GraphemeClass, PackedCell};

/// One syllable on a row: the cells from `at`, `cells` of them, a wide
/// character's tail among them counted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Syllable {
    pub at: Col,
    pub cells: Cols,
}

/// The syllables of `row`, left to right as drawn: every run of two or more
/// cells joined by a part, as the module says, where each cell's text
/// follows the one left of it -- `logical` gives the column in the text a
/// drawn column shows. A cell's marks go through the machine too, so a
/// consonant after a cell holding a virama joins it. A cell that draws
/// nothing ends what may join. A row of ASCII, CJK or any text the rules
/// never part holds none, and costs one table load a cell.
///
/// **And whether the walk met a Devanagari letter**
/// (`crate::word::is_letter`), read from the base each cell's walk asks for
/// anyway, so a row of none is known to hold no word at no further cost.
pub(crate) fn of_row(
    term: &Term<'_>,
    row: &[PackedCell],
    logical: impl Fn(Col) -> Col,
) -> (Vec<Syllable>, crate::word::Letters) {
    let mut seen = crate::word::Letters::None;
    let mut out = Vec::new();
    let mut m = ClusterBreak::after_control();
    // The open run: where it starts, where it ends, and whether it parted.
    let mut open: Option<(usize, usize, bool)> = None;
    let close = |open: &mut Option<(usize, usize, bool)>, out: &mut Vec<Syllable>| {
        if let Some((at, end, true)) = open.take() {
            out.push(Syllable {
                at: Col::at_index(at),
                cells: Cols::of_index(end - at),
            });
        }
    };
    for (x, cell) in row.iter().enumerate() {
        if cell.content.is_wide_tail() {
            if let Some(o) = open.as_mut() {
                o.1 = x + 1;
            }
            continue;
        }
        let Some(base) = crate::renderer::drawn_base(term, *cell) else {
            close(&mut open, &mut out);
            m = ClusterBreak::after_control();
            continue;
        };
        if crate::word::is_letter(base) {
            seen = crate::word::Letters::Seen;
        }
        let joined = match step(&mut m, base) {
            // The cell left of it, as drawn, is the one before it in the text.
            Step::Parts
                if open.is_some_and(|o| {
                    logical(Col::at_index(x)).index() == logical(Col::at_index(o.1 - 1)).index() + 1
                }) =>
            {
                Step::Parts
            }
            _ => Step::Starts,
        };
        match joined {
            Step::Parts => {
                if let Some(o) = open.as_mut() {
                    o.1 = x + 1;
                    o.2 = true;
                }
            }
            Step::Starts => {
                close(&mut open, &mut out);
                open = Some((x, x + 1, false));
            }
        }
        if cell.flags.is_marked() {
            for c in crate::renderer::drawn_marks(term, *cell).filter_map(Codepoint::new) {
                let _ = step(&mut m, c);
            }
        }
    }
    close(&mut open, &mut out);
    (out, seen)
}

/// **The syllables among a run of clusters** -- a composition's, laid as
/// its commit will be written: each cluster its codepoints and the cells
/// it takes, and each syllable of two or more clusters the index of its
/// first, how many it holds and the cells they take, by the rule
/// [`of_row`] reads cells by.
pub(crate) fn of_clusters<'c>(
    clusters: impl Iterator<Item = (&'c [Codepoint], usize)>,
) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut m = ClusterBreak::after_control();
    let mut open: Option<(usize, usize, usize)> = None;
    for (i, (cps, cells)) in clusters.enumerate() {
        let parts = cps.first().is_some_and(|&c| step(&mut m, c) == Step::Parts);
        for &c in cps.iter().skip(1) {
            let _ = step(&mut m, c);
        }
        match (parts, open.as_mut()) {
            (true, Some(o)) => {
                o.1 += 1;
                o.2 += cells;
            }
            _ => {
                if let Some(o) = open.take().filter(|o| o.1 > 1) {
                    out.push(o);
                }
                open = Some((i, 1, cells));
            }
        }
    }
    if let Some(o) = open.filter(|o| o.1 > 1) {
        out.push(o);
    }
    out
}

/// Whether `next` belongs to the syllable `head` ends, by the rule
/// [`of_row`] reads cells by: what a composition asks of the cell before
/// the cursor.
pub(crate) fn continues(head: &[Codepoint], next: Codepoint) -> bool {
    let mut m = ClusterBreak::after_control();
    for &c in head {
        let _ = step(&mut m, c);
    }
    step(&mut m, next) == Step::Parts
}

/// What one character does to the syllable before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// The extended rules join it where the core gave it a cell of its own
    /// (`ClusterBreak::parts_at`): it belongs to the syllable before it.
    Parts,
    /// Anything else: a boundary, or a join both variants make, which the
    /// core kept in the cell.
    Starts,
}

/// One character through the machine: ASCII classed without a table, as
/// the core's writer classes it, so a row of it costs one load of the
/// transition table a cell.
fn step(m: &mut ClusterBreak, c: Codepoint) -> Step {
    let (class, lb) = GraphemeClass::of_with_lookback(c);
    let before = *m;
    let joins = !m.is_break_classified(class, lb);
    match joins && before.parts_at(ClusterCells::Legacy, class, lb, c) {
        true => Step::Parts,
        false => Step::Starts,
    }
}

/// [`of_row`]'s syllables walked with a column that only grows: the
/// syllable starting at `c`, if one does, and whether `c` starts one. The
/// ones passed are dropped from the front.
pub(crate) struct Walk<'s> {
    left: &'s [Syllable],
}

impl<'s> Walk<'s> {
    pub(crate) fn new(all: &'s [Syllable]) -> Walk<'s> {
        Walk { left: all }
    }

    /// The syllable that starts at column `c`.
    pub(crate) fn at(&mut self, c: Col) -> Option<Syllable> {
        while let [first, rest @ ..] = self.left {
            if first.at >= c {
                break;
            }
            self.left = rest;
        }
        self.left.first().copied().filter(|s| s.at == c)
    }
}
