//! **A letter of a joining script wider than its one cell**, drawn by
//! baud's rule for a glyph wider than its cell: full size into the cells its
//! ink reaches when they are blank, else fitted to its one cell -- scaled
//! by its ink, centred, the baseline kept, its marks scaled and moved with
//! it.
//!
//! A proportional face draws an Arabic letter at its own advance, and the
//! grid gives every letter one cell. Noto Sans Arabic's medial sad is 949
//! units at an em of 1000 against a cell of 600, so drawn at the em from
//! its cell's left edge it covered the cell on its right: the dot of ض in
//! تضع, a mark the face places 576 units in, landed under the two dots of
//! the ت beside it, and the isolated feh of حرف, 983 units, covered the ر
//! before it. The shaping was HarfBuzz's to the glyph; the cell was too
//! narrow for it. A letter whose ink reaches only blank cells keeps its
//! size, as rule D draws any glyph whose next cell is blank.
//!
//! Only a letter that takes a joining form (Joining_Type D, R or L,
//! [`baud_domain::JoiningType`]) and its marks are fitted here; rule D's
//! other glyphs -- symbols, icons -- are drawn as before.

use baud_domain::TermRead;
use baud_domain::{Codepoint, Col, Content, JoiningType, PackedCell};

/// **Whether `cp` is a letter of a joining script**: one that takes a
/// joining form, Joining_Type D, R or L, as the shaper asks before it
/// shapes a run's joining features (`baud_font::Shaper::joins`).
pub(crate) fn takes_forms(cp: Codepoint) -> bool {
    matches!(
        JoiningType::of(cp.value()),
        JoiningType::DualJoining | JoiningType::RightJoining | JoiningType::LeftJoining
    )
}

/// Rule D's threshold: a glyph is "eligible over 1.1 cells" of ink.
const ELIGIBLE: f32 = 1.1;

/// **Where one cell's ink lies**, in pixels of the line it is drawn on,
/// from the cell's origin: the least left edge and the most right edge of
/// the outlines of its glyphs, each at its shaper's offset.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Ink {
    pub(crate) left: f32,
    pub(crate) right: f32,
}

impl Ink {
    /// The box of nothing, which [`Ink::with`] grows.
    pub(crate) const NONE: Ink = Ink {
        left: f32::MAX,
        right: f32::MIN,
    };

    /// This ink and a glyph's from `left` to `right`.
    pub(crate) fn with(self, left: f32, right: f32) -> Ink {
        Ink {
            left: self.left.min(left),
            right: self.right.max(right),
        }
    }

    fn width(self) -> f32 {
        self.right - self.left
    }
}

/// **A cell whose ink is over 1.1 cells wide**, and the width of the cell
/// it is drawn in, both in pixels of its line.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Overwide {
    ink: Ink,
    cell: f32,
}

/// **Whether a letter drawn alone reaches past its cell**: over 1.1
/// cells of ink, a letter of a joining script, so rule D may fit it --
/// what the cell path's map records of an entry, so the overlay judges
/// that cell against its neighbours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Overflow {
    Inside,
    Past,
}

/// Whether a cell beside an over-wide one is blank by rule D's reading:
/// "empty or a space whatever its background", a wide character's lead
/// or tail not blank, the cursor ignored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Blank {
    Yes,
    No,
}

impl Blank {
    /// `cell` read from `term`: blank when nothing was written there or a
    /// space with nothing attached was.
    pub(crate) fn of(term: &impl TermRead, cell: PackedCell) -> Blank {
        if cell.content == Content::EMPTY {
            return Blank::Yes;
        }
        let space = term.base_of(cell).as_codepoint() == Some(Codepoint::from_char(' '));
        match space && !cell.flags.is_marked() {
            true => Blank::Yes,
            false => Blank::No,
        }
    }
}

/// What rule D draws an over-wide cell as.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Fitting {
    /// At the face's size, into the blank cells its ink reaches.
    Full,
    /// Fitted to its one cell.
    Fitted(Narrow),
}

/// **A cell fitted to its one cell**: every glyph drawn at `k` times its
/// size, its pen `shift` pixels from the cell's origin plus its shaper's
/// offset times `k`, so the ink's left edge is on the cell's and its right
/// edge on the next cell's.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Narrow {
    k: f32,
    shift: f32,
}

impl Narrow {
    /// The scale a fitted glyph is drawn at, against its face's.
    pub(crate) fn factor(self) -> f32 {
        self.k
    }

    /// Where a glyph whose shaper offset is `along` pixels at the face's
    /// size goes across, in whole pixels from the cell's origin.
    pub(crate) fn across(self, along: f32) -> i32 {
        (self.shift + along * self.k).round() as i32
    }

    /// Where a glyph whose shaper offset is `up` pixels at the face's size
    /// goes up, in whole pixels from the baseline: scaled about it, so the
    /// baseline is kept.
    pub(crate) fn up(self, up: f32) -> i32 {
        (up * self.k).round() as i32
    }
}

impl Overwide {
    /// `ink` in a cell `cell` pixels wide, if it is over 1.1 cells.
    pub(crate) fn of(ink: Ink, cell: f32) -> Option<Overwide> {
        (cell > 0.0 && ink.width() > ELIGIBLE * cell).then_some(Overwide { ink, cell })
    }

    /// How many cells past its own the ink reaches on its left and on its
    /// right.
    fn reaches(self) -> (usize, usize) {
        let past = |px: f32| usize::try_from((px / self.cell).ceil() as i32).unwrap_or(0);
        (past(-self.ink.left), past(self.ink.right - self.cell))
    }

    /// **Full or fitted**, for the cell at visual column `col` of a line
    /// whose columns end before `end`: full when every cell its ink reaches
    /// exists and `blank` says it is blank, fitted otherwise -- the last
    /// column fits, as rule D has it, and so does the first for ink that
    /// reaches left. Each over-wide cell is judged on its own, whatever its
    /// neighbour is fitted as (rule D's "each on its own").
    pub(crate) fn fitting(self, col: Col, end: Col, blank: impl Fn(Col) -> Blank) -> Fitting {
        let (left, right) = self.reaches();
        let c = col.index();
        let lefts = (1..=left).map(|d| c.checked_sub(d));
        let rights = (1..=right).map(|d| Some(c + d).filter(|&x| x < end.index()));
        let full = lefts
            .chain(rights)
            .all(|x| x.is_some_and(|x| blank(Col::at_index(x)) == Blank::Yes));
        match full {
            true => Fitting::Full,
            false => Fitting::Fitted(self.narrow()),
        }
    }

    /// The scale and move that put the ink exactly across the cell.
    fn narrow(self) -> Narrow {
        let k = self.cell / self.ink.width();
        Narrow {
            k,
            shift: -self.ink.left * k,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: f32 = 10.0;

    fn wide(left: f32, right: f32) -> Option<Overwide> {
        Overwide::of(Ink { left, right }, CELL)
    }

    /// Rule D's threshold: 1.1 cells of ink is not over-wide, a pixel more
    /// is.
    #[test]
    fn a_cell_is_over_wide_past_one_and_a_tenth_cells_of_ink() {
        assert_eq!(wide(0.0, 11.0), None);
        assert!(wide(0.0, 11.5).is_some());
        assert!(wide(-1.0, 11.0).is_some());
    }

    /// Ink that reaches into the next cell is full size when that cell is
    /// blank, fitted when it is not, and fitted in the last column.
    #[test]
    fn full_size_only_into_blank_cells_that_exist() {
        let w = wide(0.0, 15.0).unwrap();
        let blank_at = |b: usize| {
            move |c: Col| match c.index() == b {
                true => Blank::Yes,
                false => Blank::No,
            }
        };
        assert_eq!(
            w.fitting(Col::at_index(3), Col::at_index(10), blank_at(4)),
            Fitting::Full
        );
        assert!(matches!(
            w.fitting(Col::at_index(3), Col::at_index(10), blank_at(2)),
            Fitting::Fitted(_)
        ));
        assert!(matches!(
            w.fitting(Col::at_index(9), Col::at_index(10), |_| Blank::Yes),
            Fitting::Fitted(_)
        ));
        // Ink that reaches left needs the cell on the left blank too, and
        // in the first column there is none.
        let both = wide(-3.0, 13.0).unwrap();
        assert!(matches!(
            both.fitting(Col::at_index(3), Col::at_index(10), blank_at(4)),
            Fitting::Fitted(_)
        ));
        assert_eq!(
            both.fitting(Col::at_index(3), Col::at_index(10), |_| Blank::Yes),
            Fitting::Full
        );
        assert!(matches!(
            both.fitting(Col::at_index(0), Col::at_index(10), |_| Blank::Yes),
            Fitting::Fitted(_)
        ));
    }

    /// Fitted, the ink runs from the cell's left edge to its right one, and
    /// an offset scales about the cell's origin and the baseline.
    #[test]
    fn a_fitted_cell_puts_its_ink_across_the_cell() {
        let Fitting::Fitted(n) =
            wide(-2.0, 18.0)
                .unwrap()
                .fitting(Col::at_index(0), Col::at_index(1), |_| Blank::No)
        else {
            panic!("fitted");
        };
        assert_eq!(n.factor(), 0.5);
        assert_eq!(n.across(-2.0), 0);
        assert_eq!(n.across(18.0), 10);
        assert_eq!(n.up(-8.0), -4);
    }
}
