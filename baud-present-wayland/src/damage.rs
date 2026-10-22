//! **What a frame changed, and what each buffer is missing.** The
//! compositor is told only the rows a frame changed: `damage_buffer` names
//! "the regions where the pending buffer is different from the current
//! surface contents" (`[[source: wayland]]`). A buffer the compositor
//! released and baud draws into again holds the frame it last showed, so
//! before it is attached it is brought up to date with every row changed
//! since -- foot's `reapply_old_damage`, which copies the last frame's
//! damage from the buffer shown last (`[[source: foot-2705e36-render]]`
//! 3197-3291), and whose new buffer has an age that forces a full repaint
//! (`[[source: foot-2705e36-shm]]` 507). Here the frame the renderer
//! keeps is the source, so a row is copied from it rather than from the
//! other buffer, and a new buffer is stale in every row.

use crate::shm::Px;

/// The pixel rows `top..bottom`, across the buffer's whole width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strip {
    pub top: Px,
    pub bottom: Px,
}

/// What a frame changed of the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Changed<'a> {
    /// Everything: the first frame, a new size, a margin refilled.
    Whole,
    /// These rows, and no pixel outside them.
    Strips(&'a [Strip]),
}

/// A set of a buffer's pixel rows, one bit a row; empty, of no rows,
/// by default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSet {
    words: Vec<u64>,
    rows: Px,
}

impl Default for RowSet {
    fn default() -> RowSet {
        RowSet::none(Px(0))
    }
}

impl RowSet {
    /// No row of `rows`.
    pub fn none(rows: Px) -> RowSet {
        RowSet {
            words: vec![0; usize::try_from(rows.0.div_ceil(64)).unwrap_or(0)],
            rows,
        }
    }

    /// Every row of `rows`.
    pub fn all(rows: Px) -> RowSet {
        let mut s = RowSet::none(rows);
        s.add(Strip {
            top: Px(0),
            bottom: rows,
        });
        s
    }

    /// The buffer's height the set is of.
    pub fn height(&self) -> Px {
        self.rows
    }

    /// The rows of `strip`, clipped to the set's.
    pub fn add(&mut self, strip: Strip) {
        for r in strip.top.0..strip.bottom.0.min(self.rows.0) {
            if let Ok(w) = usize::try_from(r / 64) {
                self.words[w] |= 1 << (r % 64);
            }
        }
    }

    /// What a frame changed, on a buffer of this set's height.
    pub fn add_changed(&mut self, changed: Changed<'_>) {
        match changed {
            Changed::Whole => self.add(Strip {
                top: Px(0),
                bottom: self.rows,
            }),
            Changed::Strips(strips) => {
                for &s in strips {
                    self.add(s);
                }
            }
        }
    }

    /// Every row `other` holds too; a set of another height is all of it.
    pub fn union(&mut self, other: &RowSet) {
        if other.rows != self.rows {
            *self = RowSet::all(self.rows);
            return;
        }
        for (a, b) in self.words.iter_mut().zip(&other.words) {
            *a |= b;
        }
    }

    pub fn clear(&mut self) {
        self.words.fill(0);
    }

    fn holds(&self, r: Px) -> bool {
        match usize::try_from(r.0 / 64) {
            Ok(w) => r.0 < self.rows.0 && self.words[w] & 1 << (r.0 % 64) != 0,
            Err(_) => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// How many rows it holds.
    pub fn count(&self) -> Px {
        Px(self.words.iter().map(|w| w.count_ones()).sum())
    }

    /// Each run of rows, top to bottom.
    pub fn runs(&self) -> impl Iterator<Item = Strip> + '_ {
        let mut r = 0;
        core::iter::from_fn(move || {
            while r < self.rows.0 && !self.holds(Px(r)) {
                r += 1;
            }
            let top = r;
            while r < self.rows.0 && self.holds(Px(r)) {
                r += 1;
            }
            (top < r).then_some(Strip {
                top: Px(top),
                bottom: Px(r),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_holds_its_strips_as_runs_and_clips_to_its_rows() {
        let strip = |top, bottom| Strip {
            top: Px(top),
            bottom: Px(bottom),
        };
        let mut s = RowSet::none(Px(130));
        assert!(s.is_empty());
        s.add_changed(Changed::Strips(&[
            strip(3, 5),
            strip(4, 70),
            strip(128, 400),
        ]));
        assert_eq!(
            s.runs().collect::<Vec<_>>(),
            [strip(3, 70), strip(128, 130)]
        );
        assert_eq!(s.count(), Px(67 + 2));
        let mut t = RowSet::none(Px(130));
        t.add(strip(0, 1));
        t.union(&s);
        assert_eq!(
            t.runs().collect::<Vec<_>>(),
            [strip(0, 1), strip(3, 70), strip(128, 130)]
        );
        t.union(&RowSet::none(Px(7)));
        assert_eq!(t, RowSet::all(Px(130)), "another height is every row");
        t.clear();
        assert!(t.is_empty());
    }
}
